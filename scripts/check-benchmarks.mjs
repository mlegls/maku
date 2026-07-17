import { readdirSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import Ajv2020 from 'ajv/dist/2020.js';
import addFormats from 'ajv-formats';

const root = resolve(import.meta.dir, '..');
const read = path => JSON.parse(readFileSync(resolve(root, path), 'utf8'));
const ajv = new Ajv2020({ allErrors: true, strict: false });
addFormats(ajv);
const workload = ajv.compile(read('bench/schema/v1/workload.schema.json'));
const result = ajv.compile(read('bench/schema/v1/result.schema.json'));
let checked = 0;
const validate = (validator, path) => {
  const data = JSON.parse(readFileSync(path, 'utf8'));
  if (!validator(data)) throw new Error(`${path}:\n${JSON.stringify(validator.errors, null, 2)}`);
  checked++;
};
for (const name of readdirSync(resolve(root, 'bench/workloads/v1')).filter(name => name.endsWith('.json')).sort()) {
  validate(workload, resolve(root, 'bench/workloads/v1', name));
}
for (const arg of process.argv.slice(2)) validate(result, resolve(arg));
console.log(`benchmark schemas OK (${checked} documents)`);
