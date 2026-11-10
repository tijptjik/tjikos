#!/usr/bin/env bun
// Opt-in local development launcher. Normal installed commands remain standalone.
import { createHash } from 'node:crypto';
import { execFileSync, spawnSync } from 'node:child_process';
import { createReadStream } from 'node:fs';
import { lstat, mkdir, readFile, readdir, readlink, realpath, rename, rm, writeFile } from 'node:fs/promises';
import { homedir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export const apps = ['mailtjik', 'notjik', 'tjikup', 'synctjik', 'greetjik', 'bartjik', 'runtjik', 'tasktjik'] as const;
export type App = typeof apps[number];
export type Context = { root: string; home: string; cache: string };
type Run = (command: string[], env?: NodeJS.ProcessEnv) => void | Promise<void>;
const payloadApps = new Set(['mailtjik', 'notjik', 'bartjik', 'runtjik', 'tasktjik']);
const sharedInputs = ['package.json', 'bun.lock', 'bunfig.toml', 'turbo.json', '.cargo', 'rust-toolchain', 'rust-toolchain.toml', 'go.work', 'go.work.sum', 'scripts', 'completions', 'docs/icons'];
const ignored = new Set(['node_modules', 'dist', 'target', '.turbo', '__pycache__', 'browser-artifacts']);

export function appName(name: string): App {
    if (!apps.includes(name as App)) throw new Error(`Unknown app: ${name}. Choose ${apps.join(', ')}.`);
    return name as App;
}

export async function workspaceRoots(root: string, app: App): Promise<string[]> {
    const packages = new Map<string, { path: string; manifest: any }>();
    for (const group of ['apps', 'packages']) {
        for (const entry of await readdir(join(root, group))) {
            const path = `${group}/${entry}`;
            try {
                const manifest = JSON.parse(await readFile(join(root, path, 'package.json'), 'utf8'));
                packages.set(manifest.name, { path, manifest });
            } catch (error: any) {
                if (error.code !== 'ENOENT' && error.code !== 'ENOTDIR') throw error;
            }
        }
    }
    const selected = new Set<string>();
    const ordered: string[] = [];
    const visit = (name: string) => {
        const pkg = packages.get(name);
        if (!pkg) throw new Error(`Missing workspace ${name}`);
        if (selected.has(pkg.path)) return;
        selected.add(pkg.path);
        const deps = { ...pkg.manifest.dependencies, ...pkg.manifest.devDependencies };
        for (const [dep, version] of Object.entries(deps)) {
            if (String(version).startsWith('workspace:')) visit(dep);
        }
        ordered.push(pkg.path);
    };
    visit(`@tjikos/${app}`);
    return ordered;
}

// Hash content and modes, not timestamps. Follow source links used by packaged QML.
export async function fingerprint(paths: string[]): Promise<string> {
    const hash = createHash('sha256');
    const visit = async (path: string, ancestors: Set<string>) => {
        const stat = await lstat(path);
        hash.update(JSON.stringify([path, stat.mode]));
        if (stat.isSymbolicLink()) {
            hash.update(await readlink(path));
            const target = await realpath(path);
            if (ancestors.has(target)) throw new Error(`Cyclic source link: ${path}`);
            await visit(target, new Set([...ancestors, target]));
        } else if (stat.isDirectory()) {
            for (const name of (await readdir(path)).sort()) await visit(join(path, name), ancestors);
        } else if (stat.isFile()) {
            for await (const chunk of createReadStream(path)) hash.update(chunk);
        } else throw new Error(`Unsupported runtime file: ${path}`);
    };
    for (const path of [...new Set(paths)].sort()) await visit(path, new Set());
    return hash.digest('hex');
}

export async function sourceFingerprint(context: Context, roots: string[]): Promise<string> {
    const listed = execFileSync('git', ['-C', context.root, 'ls-files', '--cached', '--others', '--exclude-standard', '-z', '--', ...roots, ...sharedInputs], { maxBuffer: 32 * 1024 * 1024 }).toString().split('\0').filter(Boolean);
    const files: string[] = [];
    for (const path of [...new Set(listed)].sort()) {
        if (path.split('/').some(part => ignored.has(part))) continue;
        const full = join(context.root, path);
        try { await lstat(full); files.push(full); }
        catch (error: any) { if (error.code !== 'ENOENT') throw error; } // Tracked deletion.
    }
    if (!files.length) throw new Error('No source files found in this checkout.');
    return fingerprint(files);
}

export function installedPaths(context: Context, roots: string[]): string[] {
    const prefix = join(context.home, '.local');
    const paths: string[] = [];
    for (const root of roots) {
        const name = root.split('/')[1];
        if (!root.startsWith('apps/') || !apps.includes(name as App)) continue;
        paths.push(join(prefix, 'bin', name));
        if (payloadApps.has(name)) paths.push(join(prefix, 'lib', name));
        if (name === 'notjik' || name === 'bartjik') paths.push(join(prefix, 'bin', `${name}ctl`));
        if (name === 'runtjik') paths.push(join(prefix, 'bin', 'runtjik-image'));
    }
    return paths;
}

async function deployedFingerprint(paths: string[]): Promise<string | null> {
    try { return await fingerprint(paths); }
    catch (error: any) { if (error.code === 'ENOENT') return null; throw error; }
}

export function installedEnvironment(context: Context): NodeJS.ProcessEnv {
    const env = { ...process.env };
    for (const name of ['runtjik', 'tasktjik', 'bartjik', 'notjik']) {
        env[`${name.toUpperCase()}_HOME`] = join(context.home, '.local/lib', name);
        // Per-app installation overrides must not silently deploy somewhere else.
        delete env[`${name.toUpperCase()}_INSTALL_HOME`];
        delete env[`${name.toUpperCase()}_INSTALL_CONFIG`];
    }
    delete env.BARTJIK_QML;
    delete env.MAILTJIK_BACKEND;
    return env;
}

export async function prepare(context: Context, app: App, run: Run): Promise<boolean> {
    const roots = await workspaceRoots(context.root, app);
    const outputs = installedPaths(context, roots);
    const inputs = await sourceFingerprint(context, roots);
    const installed = await deployedFingerprint(outputs);
    const receipt = join(context.cache, `${app}.json`);
    let saved: any;
    try { saved = JSON.parse(await readFile(receipt, 'utf8')); } catch { saved = null; }
    if (installed && saved?.inputs === inputs && saved?.installed === installed) return false;
    await mkdir(context.cache, { recursive: true, mode: 0o700 });
    await rm(receipt, { force: true });
    console.error(`Updating local ${app}…`);
    const env = installedEnvironment(context);
    await run(['bun', 'install', '--frozen-lockfile'], env);
    await run(['bun', 'run', 'build', `--filter=@tjikos/${app}`], env);
    if (await sourceFingerprint(context, roots) !== inputs) throw new Error('Source changed during the build; retry before installing.');
    const targets = roots.filter(path => path.startsWith('apps/')).map(path => appName(path.split('/')[1]));
    await run(['bash', 'scripts/install.sh', '--from-build', ...targets], env);
    const deployed = await deployedFingerprint(outputs);
    if (!deployed) throw new Error('Installation did not produce all required runtime files.');
    if (await sourceFingerprint(context, roots) !== inputs) throw new Error('Source changed during installation; retry to verify the installed version.');
    // Reload only desktop processes for the installed application, never archive workers.
    await refreshResident(context, app, run, env);
    const temporary = `${receipt}.${process.pid}.tmp`;
    await writeFile(temporary, JSON.stringify({ inputs, installed: await fingerprint(outputs) }) + '\n', { mode: 0o600 });
    await rename(temporary, receipt);
    return true;
}

function serviceActive(app: App): boolean {
    return ['notjik', 'bartjik'].includes(app) && spawnSync('systemctl', ['--user', 'is-active', '--quiet', `${app}.service`], { stdio: 'ignore' }).status === 0;
}
async function refreshResident(context: Context, app: App, run: Run, env: NodeJS.ProcessEnv) {
    if (!process.env.WAYLAND_DISPLAY || !['notjik', 'bartjik', 'tasktjik'].includes(app)) return;
    if (serviceActive(app)) {
        await run(['systemctl', '--user', 'restart', `${app}.service`], env);
    } else if (app === 'bartjik') {
        const binary = join(context.home, '.local/bin/bartjik');
        const live = spawnSync(binary, ['status'], { stdio: 'ignore', env });
        if (live.status === 0) await run([binary, 'quit'], env);
    } else if (app === 'notjik') {
        // Preserve a manually started notification authority and any active drafts.
        const ctl = join(context.home, '.local/bin/notjikctl');
        const live = spawnSync(ctl, ['status'], { stdio: 'ignore', env });
        if (live.status === 0) throw new Error('Installed Notjik; restart the manually launched notification daemon to use the new build.');
    } else {
        const qml = join(context.home, '.local/lib', app, 'qml');
        const live = spawnSync('quickshell', ['ipc', '--path', qml, 'show'], { stdio: 'ignore', env });
        if (live.status === 0) await run(['quickshell', 'kill', '--path', qml], env);
    }
}

export function launchArguments(app: App, args: string[]): string[] {
    if (args[0] === '--') args = args.slice(1);
    if (args.length) return args;
    if (app === 'mailtjik') return ['read'];
    if (app === 'runtjik' || app === 'tasktjik') return ['show'];
    if (app === 'tjikup' || app === 'synctjik') return ['--help'];
    return [];
}

if (import.meta.main) {
    const [first, ...rest] = process.argv.slice(2);
    if (!first || first === '--help' || first === '-h') {
        console.log(`Usage: bun run dev <${apps.join('|')}> [-- app arguments]\nChecks this checkout and the installed runtime, builds/installs when stale, then runs it.\nUse --check <app> to inspect freshness without building or launching.`);
        process.exit(first ? 0 : 2);
    }
    try {
        const internal = first === '--prepare';
        const check = first === '--check';
        const app = appName(internal || check ? rest[0] : first);
        const home = homedir();
        const context = { root: resolve(dirname(fileURLToPath(import.meta.url)), '..'), home, cache: join(process.env.XDG_CACHE_HOME || join(home, '.cache'), 'tjikos/local-run') };
        const run: Run = (command, env = installedEnvironment(context)) => {
            const result = spawnSync(command[0], command.slice(1), { cwd: context.root, env, stdio: 'inherit' });
            if (result.error) throw result.error;
            if (result.status !== 0) throw new Error(`${command[0]} failed (${result.signal || result.status}); app not launched.`);
        };
        if (check) {
            const roots = await workspaceRoots(context.root, app);
            let saved: any;
            try { saved = JSON.parse(await readFile(join(context.cache, `${app}.json`), 'utf8')); } catch {}
            const installed = await deployedFingerprint(installedPaths(context, roots));
            const current = installed && saved?.inputs === await sourceFingerprint(context, roots) && saved?.installed === installed;
            console.log(`${app}: ${current ? 'current' : 'update required'}`);
            process.exit(current ? 0 : 1);
        }
        if (internal) {
            await prepare(context, app, run);
        } else {
            await mkdir(context.cache, { recursive: true, mode: 0o700 });
            // One user-wide lock also serializes shared dependencies and other checkouts.
            await run(['flock', '--exclusive', join(context.cache, 'install.lock'), process.execPath, fileURLToPath(import.meta.url), '--prepare', app]);
            const args = launchArguments(app, rest);
            if (!args.length && serviceActive(app)) process.exit(0);
            const result = spawnSync(join(home, '.local/bin', app), args, { env: installedEnvironment(context), stdio: 'inherit' });
            if (result.error) throw result.error;
            process.exit(result.status ?? 1);
        }
    } catch (error) {
        console.error(error instanceof Error ? error.message : String(error));
        process.exit(1);
    }
}
