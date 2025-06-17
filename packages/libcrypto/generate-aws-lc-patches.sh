#!/bin/bash

git clone https://github.com/aws/aws-lc
cd aws-lc
git checkout origin/fips-2024-09-27
git format-patch --start-number=1001 --no-numbered --no-signature \
    AWS-LC-FIPS-3.0.0..

# modify the 1009-Adding-detection-of-out-of-bound-pre-bound-memory-re.patch
# to remove the section that breaks our builds

PATCH_FILE="1009-Adding-detection-of-out-of-bound-pre-bound-memory-re.patch"

awk -i inplace '
    /The efficacy of the added test was shown by changing the decrypt/ { 
        skip = 1;
        next
    }
    /(cherry picked from commit a39439bca37714c3d4090dc90f99ae876cc919b2)/ { 
        skip = 0
    }
    !skip { print }
' "$PATCH_FILE"
