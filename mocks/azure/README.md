# mocks: azure

Stands in for Azure Blob Storage. [`compose.yml`](compose.yml) runs
[Azurite](https://github.com/Azure/Azurite), Microsoft's storage emulator, with
the Blob service on `127.0.0.1:10000`. Blobs are held in memory.

```bash
docker compose -f mocks/azure/compose.yml up
mocks/azure/inspect.sh        # list containers and blobs; needs the Azure CLI
```

Subscriber entry that uploads every record to the container `first-bite`,
which the sink creates. The account name and key are the fixed development
credentials that Azurite documents; they open no real account.

```yaml
subscribers:
  - name: blob
    subscriber_type: "objectstorage"
    subtype: "azureobject"
    zmq_url: "127.0.0.1"
    zmq_port: 12000
    topics: ["*"]
    cloud_account: "devstoreaccount1"
    cloud_access_key: "Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1OUzFT50uSRZ6IFsuFq2UVErCz4I6tq/K1SZFPTOtr/KBHBeksoGMGw=="
    cloud_container: "first-bite"
    cloud_location_type: "Emulator"
    cloud_address: "127.0.0.1"
    cloud_port: 10000
    cloud_api_version: "2023-01-03"
```
