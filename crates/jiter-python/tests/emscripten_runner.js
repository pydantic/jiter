const {opendir} = require('node:fs/promises');
const {loadPyodide} = require('pyodide');
const path = require('path');

async function find_wheel(dist_dir) {
  const dir = await opendir(dist_dir);
  for await (const dirent of dir) {
    if (dirent.name.endsWith('.whl')) {
      return path.join(dist_dir, dirent.name);
    }
  }
}

async function main() {
  const root_dir = path.resolve(__dirname, '..');
  const wheel_path = await find_wheel(path.join(root_dir, 'dist'));
  let errcode = 1;
  try {
    const pyodide = await loadPyodide({
      stdout: (msg) => {
        stdout.push(msg)
      },
      stderr: (msg) => {
        stderr.push(msg)
      }
    });
    const FS = pyodide.FS;
    setupStreams(FS, pyodide._module.TTY);
    FS.mkdir('/test_dir');
    FS.mount(FS.filesystems.NODEFS, {root: path.join(root_dir, 'tests')}, '/test_dir');
    FS.chdir('/test_dir');

    // mount jiter crate source for benchmark data
    FS.mkdir('/jiter');
    FS.mount(FS.filesystems.NODEFS, {root: path.resolve(root_dir, "..", "jiter")}, '/jiter');

    await pyodide.loadPackage(['micropip', 'pytest']);
    // language=python
    errcode = await pyodide.runPythonAsync(`
import micropip
import importlib

# ugly hack to get tests to work on arm64 (my m1 mac)
# see https://github.com/pyodide/pyodide/issues/2840
# import sys; sys.setrecursionlimit(200)

await micropip.install([
  'dirty_equals',
  'file:${wheel_path}'
])
importlib.invalidate_caches()

print('installed packages:', micropip.list())

import pytest
pytest.main()
`);
  } catch (e) {
    console.error(e);
    process.exit(1);
  }
  process.exit(errcode);
}

main();
