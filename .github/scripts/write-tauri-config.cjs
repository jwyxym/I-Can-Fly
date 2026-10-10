const fs = require('node:fs');
const path = require('node:path');

const version = process.env.TAURI_BUILD_VERSION;
if (!version) {
  throw new Error('TAURI_BUILD_VERSION is required');
}

const configPath = path.resolve(__dirname, '../../src-tauri/tauri.conf.json');
const config = JSON.parse(fs.readFileSync(configPath, 'utf8'));
config.version = version;
config.bundle ??= {};
config.bundle.resources = process.platform === 'win32' ? ['xray.dll', 'wintun.dll'] : [];
fs.writeFileSync(configPath, `${JSON.stringify(config, null, 2)}\n`, 'utf8');
