# Configuration

Contains YAML configuration files for Flash. Key files:
- `flash.yaml` - Base development configuration
- `flash.docker.yaml` - Docker container configuration
- `flash.shadow.yaml` - Shadow mode validation configuration
- `flash.production.yaml` - Production deployment configuration

Credentials (`api_key` / `account_id`) are left empty in all profiles and are
expected to be supplied via environment variables at runtime.
