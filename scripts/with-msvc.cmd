@echo off
setlocal

set "LLVM_BIN=C:\Program Files\LLVM\bin"
set "MSYS64_BIN=C:\msys64\clang64\bin"
set "MSYS64_LIB=C:\msys64\clang64\lib"
set "DEFAULT_VCPKG_OPENCV=%USERPROFILE%\AppData\Local\Temp\codex-opencv-vcpkg\vcpkg_installed\x64-windows"
set "MSVC_VCPKG_OPENCV=C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\vcpkg\installed\x64-windows"

:: Try VS Build Tools vcvars first (preferred)
set "VCVARS=C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
if exist "%VCVARS%" (
    call "%VCVARS%" >nul 2>&1
) else (
    echo [with-msvc] vcvars64.bat not found at "%VCVARS%", using LLVM clang-cl fallback
)

:: LLVM clang-cl for MSVC-compatible compilation
if exist "%LLVM_BIN%\clang-cl.exe" (
    set "PATH=%LLVM_BIN%;%PATH%"
    if not defined CC set "CC=clang-cl.exe"
    if not defined CXX set "CXX=clang-cl.exe"
)

:: Prefer MSVC/vcpkg OpenCV first to keep link.exe inputs consistent (.lib names).
if not defined OPENCV_LINK_PATHS (
    if exist "%DEFAULT_VCPKG_OPENCV%\lib\opencv_core4.lib" (
        set "OPENCV_ROOT=%DEFAULT_VCPKG_OPENCV%"
    ) else if exist "%MSVC_VCPKG_OPENCV%\lib\opencv_core4.lib" (
        set "OPENCV_ROOT=%MSVC_VCPKG_OPENCV%"
    ) else if exist "%MSYS64_LIB%\pkgconfig\opencv4.pc" (
        if not defined PKG_CONFIG_PATH set "PKG_CONFIG_PATH=%MSYS64_LIB%\pkgconfig"
    )
)

if defined OPENCV_ROOT (
    if not defined OPENCV_LINK_LIBS set "OPENCV_LINK_LIBS=opencv_imgproc4,opencv_core4"
    if not defined OPENCV_LINK_PATHS set "OPENCV_LINK_PATHS=%OPENCV_ROOT%\lib"
    if not defined OPENCV_INCLUDE_PATHS set "OPENCV_INCLUDE_PATHS=%OPENCV_ROOT%\include\opencv4"
    if not defined OPENCV_DISABLE_PROBES set "OPENCV_DISABLE_PROBES=cmake,vcpkg_cmake,vcpkg,pkg_config"
    if exist "%OPENCV_ROOT%\bin" set "PATH=%OPENCV_ROOT%\bin;%PATH%"
)

if not defined OPENCV_ROOT (
    if not defined OPENCV_DISABLE_PROBES set "OPENCV_DISABLE_PROBES=cmake,vcpkg_cmake,vcpkg"
)

:: MSYS2 clang and pkg-config for opencv-rust probes
if exist "%MSYS64_BIN%\clang.exe" (
    set "PATH=%MSYS64_BIN%;%PATH%"
)
if defined PKG_CONFIG_PATH (
    set "PKG_CONFIG_PATH=%PKG_CONFIG_PATH%"
)

set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"

%*
