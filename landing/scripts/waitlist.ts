import { execFileSync } from 'node:child_process';
import { writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

interface WaitlistEntry {
  id: number;
  contact: string;
  kind: 'email' | 'phone';
  source: string | null;
  created_at: string;
}

const args = process.argv.slice(2);
const isLocal = args.includes('--local');
const isCsv = args.includes('--csv');
const isJson = args.includes('--json');
const customOut = args.find((a) => a.startsWith('--out='))?.slice(6);

const target = isLocal ? '--local' : '--remote';
const sql = 'SELECT id, contact, kind, source, created_at FROM waitlist ORDER BY id ASC;';

try {
  const raw = execFileSync(
    'bunx',
    ['wrangler', 'd1', 'execute', 'ghostpost-waitlist', target, '--json', '--command', sql],
    {
      encoding: 'utf8',
      cwd: resolve(import.meta.dirname, '..'),
      stdio: ['ignore', 'pipe', 'pipe'],
    },
  );

  const parsed = JSON.parse(raw);
  const rows: WaitlistEntry[] = parsed[0]?.results ?? [];

  if (rows.length === 0) {
    console.log(`No waitlist entries found (${target}).`);
    process.exit(0);
  }

  if (isJson) {
    const jsonOutput = JSON.stringify(rows, null, 2);
    if (customOut) {
      writeFileSync(customOut, jsonOutput + '\n');
      console.log(`Wrote ${rows.length} entries to ${customOut}`);
    } else {
      console.log(jsonOutput);
    }
    process.exit(0);
  }

  if (isCsv) {
    const header = 'id,contact,kind,source,created_at\n';
    const csvContent =
      header +
      rows
        .map(
          (r) =>
            `${r.id},"${r.contact.replace(/"/g, '""')}",${r.kind},"${(r.source ?? '').replace(/"/g, '""')}",${r.created_at}`,
        )
        .join('\n') +
      '\n';

    const outFile = customOut ?? resolve(import.meta.dirname, '../waitlist.csv');
    writeFileSync(outFile, csvContent);
    console.log(`Exported ${rows.length} waitlist entries to ${outFile}`);
    process.exit(0);
  }

  // Default: Pretty formatted table
  console.log(`\nGhostpost Waitlist (${target.replace('--', '')} - ${rows.length} entries):\n`);
  console.table(
    rows.map((r) => ({
      ID: r.id,
      Contact: r.contact,
      Type: r.kind,
      Source: r.source ?? '-',
      Joined: r.created_at,
    })),
  );
} catch (err: unknown) {
  console.error('Failed to query waitlist from D1:');
  if (err && typeof err === 'object' && 'stderr' in err && err.stderr) {
    console.error(String(err.stderr));
  } else if (err instanceof Error) {
    console.error(err.message);
  } else {
    console.error(err);
  }
  process.exit(1);
}
