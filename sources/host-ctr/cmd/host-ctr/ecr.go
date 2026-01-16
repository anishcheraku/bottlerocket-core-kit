package main

import (
	"context"
	"encoding/base64"
	"errors"
	"fmt"
	"regexp"
	"strings"

	"github.com/aws/aws-sdk-go-v2/aws"
	"github.com/aws/aws-sdk-go-v2/config"
	"github.com/aws/aws-sdk-go-v2/service/ecr"
	"github.com/aws/aws-sdk-go-v2/service/ecrpublic"
	"github.com/containerd/containerd"
	"github.com/containerd/containerd/reference"
	"github.com/containerd/containerd/remotes/docker"
	"github.com/containerd/log"
)

// specialRegionEndpoints supports regions not yet included in the AWS GO SDK.
// ap-southeast-7 is currently in the SDK but persisted here for future region builds.
var specialRegionEndpoints = map[string]string{
	"ap-southeast-7": "https://api.ecr.ap-southeast-7.amazonaws.com",
}

// ecrPrivateHostPattern matches ECR private registry hostnames.
// Capture groups: [1] = account (unused), [2] = "-fips" or empty, [3] = region
//
// ECR hostname pattern also used in the ecr-credential-provider:
// https://github.com/kubernetes/cloud-provider-aws/blob/d1c7c02d2da22e87175802ec94c73bd8871691bc/cmd/ecr-credential-provider/main.go#L46
//
// Example hostnames:
//   - 777777777777.dkr.ecr.us-west-2.amazonaws.com
//   - 777777777777.dkr.ecr-fips.us-west-2.amazonaws.com
//   - 777777777777.dkr.ecr.cn-north-1.amazonaws.com.cn
var ecrPrivateHostPattern = regexp.MustCompile(
	`^(\d{12})` + // [1] account ID (12 digits)
		`\.dkr[\.\-]ecr` + // .dkr.ecr or .dkr-ecr
		`(\-fips)?` + // [2] optional -fips
		`\.([a-zA-Z0-9][a-zA-Z0-9-_]*)` + // [3] region
		`\.(?:` + // domain suffix (non-capturing)
		`amazonaws\.(?:com(?:\.cn)?|eu)|` + // amazonaws.com, .com.cn, .eu
		`on\.(?:aws|amazonwebservices\.com\.cn)|` + // on.aws, on.amazonwebservices.com.cn
		`sc2s\.sgov\.gov|` + // Isolated regions
		`c2s\.ic\.gov|` +
		`cloud\.adc-e\.uk|` +
		`csp\.hci\.ic\.gov` +
		`)$`)

const ecrPublicHost = "public.ecr.aws"
const ecrPublicRegion = "us-east-1"

// Regions with FIPS endpoints (see "FIPS" entries at the link below)
// https://docs.aws.amazon.com/general/latest/gr/ecr.html
var fipsSupportedEcrRegionSet = map[string]bool{
	"us-east-1":     true,
	"us-east-2":     true,
	"us-west-1":     true,
	"us-west-2":     true,
	"us-gov-east-1": true,
	"us-gov-west-1": true,
}

// parsedECR contains the parsed components of an ECR private registry hostname.
type parsedECR struct {
	Region string
	Fips   bool
}

// extractHostFromRef extracts the registry hostname from an image reference.
func extractHostFromRef(ref string) (string, error) {
	parsed, err := reference.Parse(ref)
	if err != nil {
		return "", fmt.Errorf("failed to parse reference: %w", err)
	}
	return parsed.Hostname(), nil
}

// parseECRHost parses an ECR private registry hostname and extracts
// the region and whether it's a FIPS endpoint.
func parseECRHost(host string) (*parsedECR, error) {
	matches := ecrPrivateHostPattern.FindStringSubmatch(host)
	if matches == nil {
		return nil, fmt.Errorf("not a valid ECR host: %s", host)
	}

	isFips := matches[2] == "-fips"
	region := matches[3]

	if isFips {
		if _, ok := fipsSupportedEcrRegionSet[region]; !ok {
			return nil, fmt.Errorf("invalid FIPS region: %s", region)
		}
	}

	return &parsedECR{Region: region, Fips: isFips}, nil
}

// isECRPrivateRef returns true if ref points to an ECR private registry.
func isECRPrivateRef(ref string) bool {
	host, err := extractHostFromRef(ref)
	if err != nil {
		return false
	}
	return ecrPrivateHostPattern.MatchString(host)
}

// decodeECRToken decodes a base64 ECR token and returns username and password.
func decodeECRToken(token *string) (string, string, error) {
	if token == nil {
		return "", "", errors.New("missing authorization token")
	}

	authToken, err := base64.StdEncoding.DecodeString(*token)
	if err != nil {
		return "", "", fmt.Errorf("failed to decode authorization token: %w", err)
	}

	if len(authToken) == 0 {
		return "", "", errors.New("authorization token is empty after base64 decoding")
	}

	tokens := strings.SplitN(string(authToken), ":", 2)
	if len(tokens) != 2 {
		return "", "", errors.New("invalid authorization token format")
	}

	return tokens[0], tokens[1], nil
}

// getECRPrivateCredentials fetches authorization credentials for private ECR registries.
func getECRPrivateCredentials(ctx context.Context, region string, useFIPS bool) (string, string, error) {
	cfgOpts := []func(*config.LoadOptions) error{config.WithRegion(region)}

	if useFIPS {
		cfgOpts = append(cfgOpts, config.WithUseFIPSEndpoint(aws.FIPSEndpointStateEnabled))
	}

	cfg, err := config.LoadDefaultConfig(ctx, cfgOpts...)
	if err != nil {
		return "", "", fmt.Errorf("failed to load AWS config for region %s: %w", region, err)
	}

	log.G(ctx).WithField("region", region).WithField("fips", useFIPS).Info("setting up ECR client")

	var client *ecr.Client
	if endpoint, ok := specialRegionEndpoints[region]; ok {
		log.G(ctx).WithField("region", region).WithField("endpoint", endpoint).Info("using special region endpoint")
		client = ecr.NewFromConfig(cfg, func(o *ecr.Options) {
			o.BaseEndpoint = aws.String(endpoint)
		})
	} else {
		client = ecr.NewFromConfig(cfg)
	}

	output, err := client.GetAuthorizationToken(ctx, &ecr.GetAuthorizationTokenInput{})
	if err != nil {
		return "", "", fmt.Errorf("failed to get ECR authorization token: %w", err)
	}

	if output == nil || len(output.AuthorizationData) == 0 {
		return "", "", fmt.Errorf("no authorization data returned")
	}

	return decodeECRToken(output.AuthorizationData[0].AuthorizationToken)
}

// getECRPublicCredentials fetches authorization credentials for ECR Public registries using us-east-1.
func getECRPublicCredentials(ctx context.Context) (string, string, error) {
	cfg, err := config.LoadDefaultConfig(ctx, config.WithRegion(ecrPublicRegion))
	if err != nil {
		return "", "", fmt.Errorf("failed to load AWS config for ECR Public (%s): %w", ecrPublicRegion, err)
	}

	client := ecrpublic.NewFromConfig(cfg)
	output, err := client.GetAuthorizationToken(ctx, &ecrpublic.GetAuthorizationTokenInput{})
	if err != nil {
		return "", "", fmt.Errorf("failed to get ECR Public authorization token: %w", err)
	}

	if output == nil || output.AuthorizationData == nil {
		return "", "", errors.New("missing authorization data")
	}

	return decodeECRToken(output.AuthorizationData.AuthorizationToken)
}

// withECRPrivateResolver creates a resolver for private ECR registries.
// Returns an error if credentials cannot be obtained - private ECR requires
// authentication.
func withECRPrivateResolver(ctx context.Context, ref string) containerd.RemoteOpt {
	return func(_ *containerd.Client, c *containerd.RemoteContext) error {
		ecrHost, err := extractHostFromRef(ref)
		if err != nil {
			return fmt.Errorf("failed to extract host from reference: %w", err)
		}

		parsed, err := parseECRHost(ecrHost)
		if err != nil {
			return fmt.Errorf("failed to parse ECR host: %w", err)
		}

		username, password, err := getECRPrivateCredentials(ctx, parsed.Region, parsed.Fips)
		if err != nil {
			return fmt.Errorf("failed to get private ECR credentials for region %s: %w", parsed.Region, err)
		}

		authOpt := docker.WithAuthCreds(func(host string) (string, string, error) {
			if host != ecrHost {
				return "", "", fmt.Errorf("ecr-private: unexpected host %s, expected %s", host, ecrHost)
			}
			return username, password, nil
		})
		authorizer := docker.NewDockerAuthorizer(authOpt)
		c.Resolver = docker.NewResolver(docker.ResolverOptions{
			Hosts: registryHosts(nil, &authorizer),
		})

		log.G(ctx).WithField("ref", ref).WithField("region", parsed.Region).Info("pulling private ECR image")
		return nil
	}
}

// withECRPublicResolver creates a resolver for ECR Public registries.
// Falls back to unauthenticated pull if credentials cannot be obtained since
// ECR Public supports anonymous access.
func withECRPublicResolver(ctx context.Context, ref string, registryConfig *RegistryConfig, defaultResolver containerd.RemoteOpt) containerd.RemoteOpt {
	if registryConfig != nil {
		if _, found := registryConfig.Credentials[ecrPublicHost]; found {
			return defaultResolver
		}
	}

	username, password, err := getECRPublicCredentials(ctx)
	if err != nil {
		log.G(ctx).WithError(err).Warn("ecr-public: failed to get credentials, falling back to unauthenticated pull")
		return defaultResolver
	}

	authOpt := docker.WithAuthCreds(func(host string) (string, string, error) {
		if host != ecrPublicHost {
			return "", "", fmt.Errorf("ecr-public: unexpected host %s, expected %s", host, ecrPublicHost)
		}
		return username, password, nil
	})
	authorizer := docker.NewDockerAuthorizer(authOpt)

	return func(_ *containerd.Client, c *containerd.RemoteContext) error {
		c.Resolver = docker.NewResolver(docker.ResolverOptions{
			Hosts: registryHosts(registryConfig, &authorizer),
		})
		log.G(ctx).WithField("ref", ref).Info("pulling from ECR Public")
		return nil
	}
}
