#!/bin/bash

set -euv -o pipefail

# For Tari stuff, only nightly will work
channels_to_build=`cat base/rust-toolchain`
tools_to_build="${TOOLS_TO_BUILD-rustfmt clippy}"
perform_push="${PERFORM_PUSH-false}"

repository="quay.io/tarilabs"

echo "Building ${channels_to_build}"
#for channel in $channels_to_build; do
#    cd "base"
#
#    image_name="rust-${channel}"
#    full_name="${repository}/${image_name}"
#
##    docker pull "${full_name}"
#    docker build -t "${full_name}" \
#           --cache-from "${full_name}" \
#           --build-arg channel="${channel}" \
#           .
#    docker tag "${full_name}" "${image_name}"
#
#    if [[ "${perform_push}" == 'true' ]]; then
#        docker push "${full_name}"
#    fi
#
#    cd ..
#done

crate_api_base=https://crates.io/api/v1/crates

for tool in $tools_to_build; do
    cd "${tool}"

    image_name="${tool}"
    full_name="${repository}/${image_name}"

#    docker pull "${full_name}"
    docker build -t "${full_name}" \
           --cache-from "${full_name}" \
           .
    docker tag "${full_name}" "${image_name}"

    if [[ "${perform_push}" == 'true' ]]; then
        docker push "${full_name}"
    fi

    cd ..
done
