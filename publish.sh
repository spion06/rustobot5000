#!/bin/bash
# Build the bot image and push it to Harbor. The running deployment
# (moose-robot-rustobot5000) is annotated for Keel with keel.sh/policy=force +
# poll, so Keel rolls it automatically within ~1 min of the push — no kubectl step.
#
#   ./publish.sh [tag]     (tag defaults to "test", matching moose-robot-values.yaml)
set -euo pipefail
cd "$(dirname "$0")"

IMAGE="harbor.zenacra.net:32301/rustobot5000/main"
TAG="${1:-test}"

buildah build --layers -t "${IMAGE}:${TAG}" .
buildah push "${IMAGE}:${TAG}"

echo
echo "pushed ${IMAGE}:${TAG}"
echo "Keel will roll the deployment shortly. Watch with:"
echo "  kubectl -n default rollout status deployment/moose-robot-rustobot5000 -w"
