# Dependency Distribution

This document explains how the plugin handles OpenMP (libgomp) dependency distribution to ensure it works on all Linux systems without requiring users to install system packages.

## Problem

OpenMP parallelization requires `libgomp.so.1`, but not all systems have it installed:

```
Error: libgomp.so.1: cannot open shared object file: No such file or directory
```

This breaks the plugin for users without OpenMP libraries installed (see [Issue #48](https://github.com/esmuellert/codediff.nvim/issues/48)).

---

## Solution: RPATH + Conditional Bundling

### Overview

```
┌────────────────────────────────────────────────────────┐
│  Build Process (GitHub Actions)                        │
├────────────────────────────────────────────────────────┤
│  1. Build in CentOS 7 (GLIBC 2.17 - old & compatible)  │
│  2. Set RUNPATH=$ORIGIN in binary                      │
│  3. Copy libgomp.so.1 from CentOS 7                    │
│  4. Upload both to GitHub releases                     │
└────────────────────────────────────────────────────────┘
                          ↓
┌────────────────────────────────────────────────────────┐
│  Installation (installer.lua)                          │
├────────────────────────────────────────────────────────┤
│  1. Download libvscode_diff binary                     │
│  2. Check: Does system have libgomp?                   │
│     ├─ Yes → Done! Uses system version                 │
│     └─ No  → Download bundled libgomp                  │
└────────────────────────────────────────────────────────┘
                          ↓
┌────────────────────────────────────────────────────────┐
│  Runtime (Dynamic Linker)                              │
├────────────────────────────────────────────────────────┤
│  RUNPATH=$ORIGIN search order:                         │
│  1. Check plugin folder (./libgomp.so.1)               │
│  2. Fallback to system paths (/lib, /usr/lib)          │
│  → Uses whichever exists                               │
└────────────────────────────────────────────────────────┘
```

### How RPATH Works

The binary is built with `RUNPATH=$ORIGIN`, which tells the dynamic linker to search for dependencies relative to the binary location:

```bash
# Verify RPATH configuration
$ readelf -d libvscode_diff.so | grep RUNPATH
 0x000000000000001d (RUNPATH)  Library runpath: [$ORIGIN]
```

**Dynamic Linker Search Order:**
1. `RUNPATH` paths (if set) → checks `$ORIGIN` (plugin folder)
2. System default paths → `/lib`, `/usr/lib`, etc.
3. Error if not found

**Result:** If `libgomp.so.1` exists in plugin folder, use it; otherwise use system library.

---

## Implementation

### Build System (CMakeLists.txt)

```cmake
# Set RPATH to find bundled libgomp.so.1
if(USE_OPENMP AND UNIX AND NOT APPLE)
    set_target_properties(vscode_diff PROPERTIES
        BUILD_WITH_INSTALL_RPATH TRUE
        INSTALL_RPATH "$ORIGIN"
    )
endif()
```

### GitHub Actions Workflow

**build-and-test.yml** (Linux builds only):
```yaml
- name: Build library (Ubuntu in CentOS 7 Docker)
  run: |
    docker run --rm -v $PWD:/work centos:7 bash -c "
      # ... build commands ...
      make build
      # Copy libgomp from CentOS 7 (old GLIBC)
      cp /lib64/libgomp.so.1 ./
    "

- name: Upload build artifacts
  uses: actions/upload-artifact@v4
  with:
    path: |
      libvscode_diff.so
      libgomp.so.1
```

**release.yml** (rename with version and architecture):
```yaml
- name: Prepare release assets
  run: |
    VERSION=$(cat VERSION)
    # Rename for release
    cp libvscode_diff.so release/libvscode_diff_linux_x64_${VERSION}.so
    cp libgomp.so.1 release/libgomp_linux_x64_${VERSION}.so.1
```

### Automatic Installation (installer.lua)

Two new functions handle libgomp detection and installation:

#### 1. System Detection

```lua
local function check_system_libgomp()
  -- Only check on Linux
  if os_name ~= "linux" then
    return true
  end
  
  -- Try to load libgomp using FFI
  local ffi = require("ffi")
  local ok = pcall(function()
    ffi.load("gomp", true)
  end)
  
  return ok
end
```

#### 2. Conditional Download

```lua
local function install_libgomp_if_needed(opts)
  -- Check if already bundled
  if bundled then return true end
  
  -- Check if system has it
  if check_system_libgomp() then
    return true  -- No need to bundle
  end
  
  -- Download from releases
  local url = "https://github.com/.../libgomp_linux_{arch}_{version}.so.1"
  download_file(url, "libgomp.so.1")
  
  return true  -- Never fail installation
end
```

Called automatically after main library installation:

```lua
function M.install(opts)
  -- ... install main library ...
  
  -- Also check and install libgomp if needed
  install_libgomp_if_needed(opts)
  
  return true
end
```

---

## File Layout

After installation, the plugin folder contains:

```
~/.local/share/nvim/lazy/codediff.nvim/
├── libvscode_diff_linux_arm64_0.11.1.so   # Main binary (RUNPATH=$ORIGIN)
├── libgomp.so.1                           # Bundled (only if system doesn't have it)
├── lua/
│   └── vscode-diff/
│       └── installer.lua                  # Handles automatic download
└── plugin/
    └── vscode-diff.lua
```

---

## User Experience

### Scenario 1: System Has libgomp ✅

```
User installs plugin
  ↓
installer.lua: check_system_libgomp() → true
  ↓
"System libgomp.so.1 found, no need to bundle"
  ↓
Plugin uses system library via RPATH fallback
  ↓
Works perfectly! (no download needed)
```

### Scenario 2: System Missing libgomp ✅

```
User installs plugin
  ↓
installer.lua: check_system_libgomp() → false
  ↓
"System libgomp.so.1 not found, downloading..."
  ↓
Downloads libgomp_linux_{arch}_{version}.so.1
  ↓
Renames to libgomp.so.1 in plugin folder
  ↓
Binary finds it via RUNPATH=$ORIGIN
  ↓
Works perfectly! (Issue #48 fixed!)
```

### Scenario 3: Download Fails (Graceful Degradation) ⚠️

```
User installs plugin
  ↓
installer.lua: check_system_libgomp() → false
  ↓
Attempts download → fails (network issue, etc.)
  ↓
⚠️  Warning: "Plugin may not work without libgomp installed"
  ↓
Installation continues (doesn't fail)
  ↓
Same as before - user needs to install libgomp manually
```

---

## Testing

### Verify RPATH Configuration

```bash
# Check RUNPATH is set
readelf -d libvscode_diff.so | grep RUNPATH
# Should show: Library runpath: [$ORIGIN]
```

### Test Fallback Behavior

```bash
# Without bundled libgomp - uses system
rm libgomp.so.1
ldd libvscode_diff.so
# Output: libgomp.so.1 => /lib/x86_64-linux-gnu/libgomp.so.1

# With bundled libgomp - uses local
cp /lib64/libgomp.so.1 ./
ldd libvscode_diff.so
# Output: libgomp.so.1 => /path/to/plugin/libgomp.so.1
```

### Test in Clean Environment

Simulate a system without libgomp:

```bash
docker run --rm -it -v $(pwd):/plugin ubuntu:latest bash -c "
  apt update && apt install -y neovim curl
  # Don't install libgomp1 package
  cd /plugin
  nvim --headless -c 'lua require(\"vscode-diff\").setup()' -c quit
"
```

If automatic download works, plugin will function correctly.

---

## Benefits

✅ **Zero Configuration** - Works out of the box for all users  
✅ **Maximum Compatibility** - CentOS 7 GLIBC 2.17 works everywhere  
✅ **Minimal Overhead** - Only downloads if needed (~400KB)  
✅ **Graceful Degradation** - Uses system library when available  
✅ **Non-Breaking** - Existing working users unaffected  
✅ **Industry Standard** - RPATH with `$ORIGIN` is widely used  

---

## Technical References

### RPATH Best Practices

- **Debian RPATH Policy**: https://wiki.debian.org/RpathIssue
- **Relocatable Binaries**: Use `$ORIGIN` for plugin/bundle distributions
- **Security**: RPATH doesn't pose security risks for user-installed plugins

### Dynamic Linker Search Order

1. `RPATH` (if `DT_RPATH` set, deprecated)
2. `LD_LIBRARY_PATH` environment variable
3. `RUNPATH` (if `DT_RUNPATH` set, modern)
4. `/etc/ld.so.cache` (system libraries)
5. Default paths: `/lib`, `/usr/lib`

### CentOS 7 Compatibility

- **GLIBC Version**: 2.17 (released 2012)
- **Forward Compatible**: Binaries work on all newer systems
- **Industry Standard**: Used by Python manylinux, Node native modules

---

## Troubleshooting

### Plugin fails with "libgomp.so.1: cannot open shared object file"

**Cause**: System doesn't have libgomp, and automatic download failed.

**Solution**:
```bash
# Option 1: Install system package
sudo apt install libgomp1          # Debian/Ubuntu
sudo yum install libgomp           # CentOS/RHEL
sudo pacman -S gcc-libs            # Arch Linux

# Option 2: Manual download
cd ~/.local/share/nvim/lazy/codediff.nvim
curl -LO https://github.com/esmuellert/codediff.nvim/releases/download/v{VERSION}/libgomp_linux_{arch}_{VERSION}.so.1
mv libgomp_linux_* libgomp.so.1
```

### How to check if libgomp is installed?

```bash
# Method 1: Check for library file
ls /lib*/libgomp.so.1 /usr/lib*/libgomp.so.1

# Method 2: Use ldconfig
ldconfig -p | grep libgomp

# Method 3: Test with Lua FFI (same as installer.lua)
nvim --headless -c "lua print(pcall(function() require('ffi').load('gomp', true) end))" -c quit
```

### Verify plugin is using bundled vs system libgomp

```bash
cd ~/.local/share/nvim/lazy/codediff.nvim
ldd libvscode_diff*.so | grep gomp
```

Expected output:
- **System**: `libgomp.so.1 => /lib/x86_64-linux-gnu/libgomp.so.1`
- **Bundled**: `libgomp.so.1 => /path/to/plugin/libgomp.so.1`

---

## See Also

- [Build System Guide](BUILD.md) - CMake build configuration
- [Version Management](VERSION_MANAGEMENT.md) - Release process
- [Performance](performance.md) - OpenMP parallelization benefits
