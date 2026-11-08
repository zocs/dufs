import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { chmod, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import net from 'node:net';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { after, before, test } from 'node:test';
import { setTimeout as delay } from 'node:timers/promises';
import { fileURLToPath } from 'node:url';
import puppeteer from 'puppeteer';

const repository = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const executable = process.env.DUFS_BIN || join(repository, 'target/debug/dufs');
let browser;

before(async () => {
  browser = await puppeteer.launch({
    executablePath: process.env.PUPPETEER_EXECUTABLE_PATH || undefined,
    headless: true,
    // Ubuntu CI may disable unprivileged user namespaces. Tests only open
    // the isolated local fixture server and never visit external pages.
    args: ['--no-sandbox'],
  });
});

after(async () => {
  if (browser) await browser.close();
});

async function availablePort() {
  const socket = net.createServer();
  socket.listen(0, '127.0.0.1');
  await once(socket, 'listening');
  const port = socket.address().port;
  await new Promise((resolve, reject) => socket.close(err => err ? reject(err) : resolve()));
  return port;
}

async function fixture(t, flags, content, mode = 'edit') {
  const root = await mkdtemp(join(tmpdir(), 'dufs-editor-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  const file = join(root, 'document.txt');
  await writeFile(file, content);
  const port = await availablePort();
  const base = `http://127.0.0.1:${port}`;
  const server = spawn(executable, [
    '--bind', '127.0.0.1', '--port', String(port),
    '--assets', join(repository, 'assets'), ...flags, root,
  ], { stdio: ['ignore', 'pipe', 'pipe'] });
  let output = '';
  server.stdout.on('data', chunk => { output += chunk; });
  server.stderr.on('data', chunk => { output += chunk; });
  let spawnError;
  server.on('error', err => { spawnError = err; });
  t.after(async () => {
    if (server.exitCode === null && server.signalCode === null && !spawnError) {
      const exited = once(server, 'exit');
      server.kill('SIGTERM');
      await exited;
    }
  });
  const started = Date.now();
  for (;;) {
    if (spawnError) throw spawnError;
    assert.equal(server.exitCode, null, `dufs exited during startup: ${output}`);
    try {
      const res = await fetch(`${base}/document.txt`);
      if (res.ok) break;
    } catch {}
    assert.ok(Date.now() - started < 10000, `dufs did not start: ${output}`);
    await delay(30);
  }
  const page = await browser.newPage();
  t.after(() => page.close());
  page.setDefaultTimeout(5000);
  await page.goto(`${base}/document.txt?${mode}`, { waitUntil: 'networkidle0' });
  await page.waitForSelector('#editor', { visible: true });
  await page.waitForFunction(expected => document.querySelector('#editor').value === expected, {}, content);
  return { page, file };
}

async function typeDraft(page, value) {
  await page.focus('#editor');
  await selectAll(page);
  await page.keyboard.type(value);
  await page.$eval('#editor', el => el.setSelectionRange(2, 7));
}

async function selectAll(page) {
  await page.keyboard.down('Control');
  await page.keyboard.press('a');
  await page.keyboard.up('Control');
}

async function editorState(page) {
  return page.$eval('#editor', el => ({
    value: el.value,
    readOnly: el.readOnly,
    focused: document.activeElement === el,
    start: el.selectionStart,
    end: el.selectionEnd,
  }));
}

for (const [name, mode, flags] of [
  ['View remains read-only even when uploads are enabled', 'view', ['--allow-upload', '--allow-delete']],
  ['Edit remains read-only when uploads are disabled', 'edit', []],
]) {
  test(name, async t => {
    const { page } = await fixture(t, flags, 'original text', mode);
    await page.focus('#editor');
    await selectAll(page);
    await page.keyboard.type('should not replace the original');
    const state = await editorState(page);
    assert.equal(state.value, 'original text', 'typing must not modify a read-only document');
    assert.equal(state.readOnly, true);
    assert.equal(await page.$eval('.save-btn', el => el.classList.contains('hidden')), true);
  });
}

for (const status of [403, 500]) {
  test(`HTTP ${status} save retains the draft, selection, and editor focus`, async t => {
    const flags = status === 403 ? ['--allow-upload'] : ['--allow-upload', '--allow-delete'];
    const { page, file } = await fixture(t, flags, '');
    const draft = 'unsaved local draft';
    await typeDraft(page, draft);
    await writeFile(file, 'server update');
    if (status === 500) {
      assert.notEqual(process.getuid?.(), 0, 'permission failure fixture requires a non-root user');
      await chmod(file, 0o444);
      t.after(() => chmod(file, 0o644).catch(() => {}));
    }
    let navigations = 0;
    page.on('framenavigated', frame => {
      if (frame === page.mainFrame()) navigations += 1;
    });
    const dialogs = [];
    page.on('dialog', async dialog => {
      dialogs.push(dialog.message());
      await dialog.accept();
    });
    const put = page.waitForResponse(res => res.request().method() === 'PUT');
    await page.click('.save-btn');
    const response = await put;
    assert.equal(response.status(), status, 'fixture must produce the intended real HTTP failure');
    await page.waitForNetworkIdle({ idleTime: 300 });
    const state = await editorState(page);
    assert.equal(state.value, draft, 'failed saves must retain the unsaved draft');
    assert.equal(navigations, 0, 'failed saves must not reload the editor');
    assert.equal(dialogs.length, 1, 'failed saves must display the existing error alert');
    assert.match(dialogs[0], /^Failed to save file, /);
    assert.equal(state.focused, true, 'dismissing the error must return focus to the editor');
    assert.deepEqual([state.start, state.end], [2, 7]);
    assert.equal(await readFile(file, 'utf8'), 'server update');
  });
}

test('successful save writes the draft and reloads the editor', async t => {
  const { page, file } = await fixture(t, ['--allow-upload', '--allow-delete'], 'original text');
  const draft = 'saved document';
  await typeDraft(page, draft);
  const put = page.waitForResponse(res => res.request().method() === 'PUT');
  const navigation = page.waitForNavigation({ waitUntil: 'networkidle0' });
  await page.click('.save-btn');
  assert.equal((await put).status(), 201);
  await navigation;
  assert.equal(await readFile(file, 'utf8'), draft);
  assert.equal((await editorState(page)).value, draft);
});
