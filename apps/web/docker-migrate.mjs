import { access } from 'node:fs/promises';
import { constants } from 'node:fs';
import { join } from 'node:path';
import { spawn } from 'node:child_process';

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function run(cmd, args, opts = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(cmd, args, {
      stdio: 'inherit',
      env: process.env,
      cwd: process.cwd(),
      ...opts,
    });

    child.on('error', reject);
    child.on('exit', (code, signal) => {
      if (code === 0) {
        resolve();
        return;
      }
      reject(new Error(`${cmd} ${args.join(' ')} exited with code ${code}${signal ? ` (signal ${signal})` : ''}`));
    });
  });
}

async function main() {
  if (!process.env.DATABASE_URL) {
    console.log('DATABASE_URL not set; skipping web migrations.');
    return;
  }

  const prismaCli = join(process.cwd(), 'node_modules', 'prisma', 'build', 'index.js');
  await access(prismaCli, constants.R_OK);

  let deployed = false;
  for (let attempt = 1; attempt <= 20; attempt += 1) {
    try {
      await run(process.execPath, [prismaCli, 'migrate', 'deploy']);
      deployed = true;
      break;
    } catch (err) {
      if (attempt >= 20) throw err;
      console.error(`prisma migrate deploy failed on attempt ${attempt}/20; retrying in 2s`);
      await sleep(2000);
    }
  }

  if (!deployed) {
    throw new Error('prisma migrate deploy never succeeded');
  }

  try {
    await run(process.execPath, ['prisma/seed.js']);
  } catch (err) {
    console.error('seed step failed; continuing anyway');
    console.error(err instanceof Error ? err.message : err);
  }
}

main().catch((err) => {
  console.error(err instanceof Error ? err.stack ?? err.message : err);
  process.exit(1);
});
