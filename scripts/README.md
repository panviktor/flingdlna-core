# Build Scripts

## FFI Library Build Scripts

### `build-ffi.sh` - Production Build
```bash
./scripts/build-ffi.sh
```

Builds optimized **release** version of the FFI library:
- Optimized for performance
- Smaller binary size
- **Minimal logging**: only `info/warn/error` from our crates
- Filters out noisy libraries (mdns_sd, hyper, tokio, tower)
- Includes Chromecast support (always enabled)
- Use this for production and App Store builds

### `build-ffi-debug.sh` - Development Build
```bash
./scripts/build-ffi-debug.sh
```

Builds **debug** version of the FFI library:
- Includes debug symbols
- **Verbose logging**: `debug` level from our crates
- Shows detailed logs from dlna-core, dlna-server, dlna-combo
- Still filters excessive noise from mdns_sd and other libraries
- Use this during development for troubleshooting

## Logging Behavior

### Release Build (production)
```
INFO  dlna_core::ssdp: Starting SSDP service
INFO  dlna_server: Media server started on port 8080
WARN  dlna_combo: Device discovery timed out
```

### Debug Build (development)
```
DEBUG dlna_core::ssdp: Sending M-SEARCH for MediaRenderer
DEBUG dlna_server::http: Processing description.xml request
DEBUG dlna_combo::controller: Device state updated
INFO  dlna_core: Connected to device
```

Note: Even in debug builds, mdns_sd, hyper, tokio logs are suppressed to `warn` level to avoid console spam.
