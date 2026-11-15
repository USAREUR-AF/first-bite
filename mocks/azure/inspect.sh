#!/usr/bin/env bash
# List the containers of the Azurite emulator and the blobs in one of them.
#
#   mocks/azure/inspect.sh [container]      default container: first-bite
#
# Needs the Azure CLI (`az`). The account name and key are the fixed
# development credentials that Azurite documents; they open no real account.
set -euo pipefail

container="${1:-first-bite}"
connection="DefaultEndpointsProtocol=http;AccountName=devstoreaccount1;AccountKey=Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1OUzFT50uSRZ6IFsuFq2UVErCz4I6tq/K1SZFPTOtr/KBHBeksoGMGw==;BlobEndpoint=http://127.0.0.1:10000/devstoreaccount1;"

az storage container list --connection-string "$connection" --output table
az storage blob list --container-name "$container" --connection-string "$connection" --output table
