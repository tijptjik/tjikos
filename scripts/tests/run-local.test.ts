import { afterEach, describe, expect, test } from 'bun:test';
import { execFileSync } from 'node:child_process';
import { chmod, mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { apps, appName, installedPaths, launchArguments, prepare, sourceFingerprint, workspaceRoots, type Context } from '../run-local';

const temporary: string[] = [];
afterEach(async () => { for (const path of temporary.splice(0)) await rm(path, { recursive: true, force: true }); });
async function put(path: string, content: string) { await mkdir(resolve(path, '..'), { recursive: true }); await writeFile(path, content); }
async function fixture() {
    const base = await mkdtemp(join(tmpdir(), 'tjikos-local-run-'));
    temporary.push(base);
    const context: Context = { root: join(base, 'repo'), home: join(base, 'home'), cache: join(base, 'home/cache/tjikos/local-run') };
    await mkdir(join(context.root, 'packages'), { recursive: true });
    await put(join(context.root, 'apps/runtjik/package.json'), JSON.stringify({ name: '@tjikos/runtjik', dependencies: { '@tjikos/tasktjik': 'workspace:*' } }));
    await put(join(context.root, 'apps/tasktjik/package.json'), JSON.stringify({ name: '@tjikos/tasktjik' }));
    await put(join(context.root, 'apps/runtjik/src/main.rs'), 'launcher');
    await put(join(context.root, 'apps/tasktjik/src/lib.rs'), 'dependency');
    await put(join(context.root, 'package.json'), '{}');
    await put(join(context.root, '.gitignore'), 'node_modules/\n**/dist/\n**/target/\n');
    execFileSync('git', ['init', '-q', context.root]);
    const commands: string[][] = [];
    const run = async (command: string[]) => {
        commands.push(command);
        if (command[0] !== 'bash') return;
        for (const path of installedPaths(context, await workspaceRoots(context.root, 'runtjik'))) {
            if (path.includes('/lib/')) await put(join(path, 'payload'), 'installed');
            else { await put(path, '#!/bin/sh\nexit 0\n'); await chmod(path, 0o755); }
        }
    };
    return { context, run, commands };
}

describe('local development freshness', () => {
    test('cold install, hot skip, changed dependency and missing installed payload', async () => {
        const { context, run, commands } = await fixture();
        expect(await prepare(context, 'runtjik', run)).toBe(true);
        expect(commands.map(c => c[0])).toEqual(['bun', 'bun', 'bash']);
        expect(commands[1]).toContain('--filter=@tjikos/runtjik');
        expect(commands[2]).toEqual(['bash', 'scripts/install.sh', '--from-build', 'tasktjik', 'runtjik']);
        commands.length = 0;
        expect(await prepare(context, 'runtjik', run)).toBe(false);
        expect(commands).toEqual([]);
        await put(join(context.root, 'apps/tasktjik/src/lib.rs'), 'updated dependency');
        expect(await prepare(context, 'runtjik', run)).toBe(true);
        await rm(join(context.home, '.local/bin/runtjik-image'));
        expect(await prepare(context, 'runtjik', run)).toBe(true);
        await put(join(context.home, '.local/lib/runtjik/payload'), 'external deployment');
        expect(await prepare(context, 'runtjik', run)).toBe(true);
    });

    test('uncommitted additions and deletions count; generated files and unrelated apps do not', async () => {
        const { context } = await fixture();
        execFileSync('git', ['-C', context.root, 'add', '.']);
        const roots = await workspaceRoots(context.root, 'runtjik');
        const original = await sourceFingerprint(context, roots);
        await put(join(context.root, 'apps/runtjik/dist/generated'), 'build output');
        await put(join(context.root, 'apps/mailtjik/unrelated'), 'other app');
        expect(await sourceFingerprint(context, roots)).toBe(original);
        await put(join(context.root, 'apps/runtjik/qml/NewPane.qml'), 'new UI');
        expect(await sourceFingerprint(context, roots)).not.toBe(original);
        await rm(join(context.root, 'apps/runtjik/qml/NewPane.qml'));
        await rm(join(context.root, 'apps/runtjik/src/main.rs'));
        expect(await sourceFingerprint(context, roots)).not.toBe(original);
    });

    test('failed builds and moving sources never install or receive a fresh receipt', async () => {
        const { context, run } = await fixture();
        await prepare(context, 'runtjik', run);
        await put(join(context.root, 'apps/runtjik/src/main.rs'), 'changed');
        await expect(prepare(context, 'runtjik', () => { throw new Error('build failed'); })).rejects.toThrow('build failed');
        expect(await Bun.file(join(context.cache, 'runtjik.json')).exists()).toBe(false);
        let installed = false;
        await expect(prepare(context, 'runtjik', async command => {
            if (command[1] === 'run') await put(join(context.root, 'apps/runtjik/src/main.rs'), 'changed again during build');
            if (command[0] === 'bash') installed = true;
        })).rejects.toThrow('Source changed during the build');
        expect(installed).toBe(false);
        expect(await Bun.file(join(context.cache, 'runtjik.json')).exists()).toBe(false);
    });

    test('serializes concurrent invocations and forwards arguments literally', async () => {
        const { context } = await fixture();
        await put(join(context.root, 'scripts/run-local.ts'), await readFile(new URL('../run-local.ts', import.meta.url), 'utf8'));
        const fakeBin = join(context.home, 'test-bin');
        await put(join(fakeBin, 'bun'), '#!/bin/sh\nprintf "%s\\n" "$*" >> "$HOME/build.log"\nsleep .1\n');
        await chmod(join(fakeBin, 'bun'), 0o755);
        await put(join(context.root, 'scripts/install.sh'), `#!/bin/sh
printf 'install\\n' >> "$HOME/install.log"
mkdir -p "$HOME/.local/bin" "$HOME/.local/lib/runtjik" "$HOME/.local/lib/tasktjik"
printf 'payload' > "$HOME/.local/lib/runtjik/payload"
printf 'payload' > "$HOME/.local/lib/tasktjik/payload"
for name in runtjik runtjik-image tasktjik; do
    printf '#!/bin/sh\\nprintf "%%s\\\\n" "$@" >> "$HOME/launch.log"\\n' > "$HOME/.local/bin/$name"
    chmod +x "$HOME/.local/bin/$name"
done
`);
        const argument = 'literal $(not-a-command) with spaces';
        const argv = [process.execPath, join(context.root, 'scripts/run-local.ts'), 'runtjik', '--', argument];
        const env = { ...process.env, HOME: context.home, XDG_CACHE_HOME: join(context.home, 'cache'), PATH: `${fakeBin}:${process.env.PATH}`, WAYLAND_DISPLAY: '' };
        const children = [Bun.spawn(argv, { env, stdout: 'pipe', stderr: 'pipe' }), Bun.spawn(argv, { env, stdout: 'pipe', stderr: 'pipe' })];
        for (const child of children) {
            const result = await child.exited;
            const error = await new Response(child.stderr).text();
            expect(result, error).toBe(0);
        }
        expect(await readFile(join(context.home, 'install.log'), 'utf8')).toBe('install\n');
        expect(await readFile(join(context.home, 'launch.log'), 'utf8')).toBe(`${argument}\n${argument}\n`);
    });

    test('default install includes every app, explicit defaults avoid destructive CLI actions', async () => {
        const installer = await readFile(new URL('../install.sh', import.meta.url), 'utf8');
        const defaults = installer.match(/set -- ([a-z ]+)\n/)![1].split(' ');
        expect(defaults.sort()).toEqual([...apps].sort());
        expect(launchArguments('mailtjik', [])).toEqual(['read']);
        expect(launchArguments('tjikup', [])).toEqual(['--help']);
        expect(launchArguments('synctjik', [])).toEqual(['--help']);
        expect(launchArguments('runtjik', [])).toEqual(['show']);
        expect(() => appName('../escape')).toThrow('Unknown app');
    });
});

describe('local artifact installation', () => {
    for (const app of ['tjikup', 'synctjik']) {
        test(`${app} --from-build installs dist without fetching a release`, async () => {
            const base = await mkdtemp(join(tmpdir(), 'tjikos-source-install-'));
            temporary.push(base);
            const root = join(base, 'repo');
            const prefix = join(base, 'prefix');
            const appRoot = join(root, 'apps', app);
            const installer = join(appRoot, 'scripts/install.sh');
            await put(installer, await readFile(new URL(`../../apps/${app}/scripts/install.sh`, import.meta.url), 'utf8'));
            await put(join(appRoot, 'scripts/fetch-release.sh'), '#!/bin/sh\necho unexpected release fetch >&2\nexit 91\n');
            const capabilities = { encrypted_credentials: 1, bootstrap_archives: 1, native_commands: 1, native_installers: 1, native_maintenance: 1, scheduled_telegram: 1, scheduled_telegram_bws: 1, native_backends: 1, protection_backends: 1 };
            const binary = `#!/bin/sh\nprintf '%s\\n' '${JSON.stringify(capabilities)}'\n`;
            await put(join(appRoot, 'dist', app), binary);
            await chmod(join(appRoot, 'dist', app), 0o755);
            await put(join(root, 'scripts/install-completions.sh'), '#!/bin/sh\nexit 0\n');
            await put(join(appRoot, 'assets/tjikup.png'), 'fixture');
            await put(join(base, 'tools/gtk-update-icon-cache'), '#!/bin/sh\nexit 0\n');
            await chmod(join(base, 'tools/gtk-update-icon-cache'), 0o755);
            const env = { ...process.env, HOME: join(base, 'home'), TJIKUP_MANAGED_CONFIG: '1', TJIKUP_INSTALL_CONFIG: join(base, 'missing-config'), PATH: `${join(base, 'tools')}:${process.env.PATH}` };
            execFileSync('bash', [installer, '--from-build', prefix], { env });
            expect(await readFile(join(prefix, 'bin', app), 'utf8')).toBe(binary);
            // The normal release path remains unchanged.
            expect(() => execFileSync('bash', [installer, prefix], { env, stdio: 'pipe' })).toThrow();
            expect(await readFile(join(prefix, 'bin', app), 'utf8')).toBe(binary);
        });
    }
});
