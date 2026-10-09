@echo off
REM Builds and opens the Bevy sandbox (buildings district, hammer, demolition panel).
REM Usage: Play.bat [release] [building name]
REM   release        optimized build (much faster with many buildings; first build takes a while)
REM   building name  start in front of that lot, e.g. Play.bat 0203house01_a
REM Demolition runs are started from the ImGui "Buildings" window (Esc frees the cursor).

setlocal
pushd "%~dp0"

set "PROFILE=debug"
set "CARGO_FLAGS="
set "RFG_START_LOT="
for %%A in (%*) do (
    if /i "%%~A"=="release" (
        set "PROFILE=release"
        set "CARGO_FLAGS=--release"
    ) else (
        set "RFG_START_LOT=%%~A"
    )
)

REM The game assets are not part of this repository; they are extracted from your own game install.
if exist "LocalData\venv\Scripts\python.exe" (
    "LocalData\venv\Scripts\python.exe" "Extractors\asset_status.py"
    if not errorlevel 1 goto :assetsok
)
echo The game assets have not been extracted yet ^(LocalData\Assets is missing or incomplete^).
choice /m "Run ExtractAssets.bat now"
if errorlevel 2 goto :fail
set "RFG_NOPAUSE=1"
call "%~dp0ExtractAssets.bat"
if errorlevel 1 goto :fail
"LocalData\venv\Scripts\python.exe" "Extractors\asset_status.py"
if errorlevel 1 goto :fail
:assetsok

REM Rust installs to %USERPROFILE%\.cargo\bin, which a double-clicked .bat may not have on PATH.
if defined CARGO_HOME set "PATH=%CARGO_HOME%\bin;%PATH%"
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"

where cargo >nul 2>nul
if errorlevel 1 goto :nocargo

REM Building uses one CPU core per job. Ask how many to use (default 4), clamped to 1..all cores.
REM Set CARGO_BUILD_JOBS beforehand to skip the prompt.
if defined CARGO_BUILD_JOBS goto :build
set "MAX_CORES=%NUMBER_OF_PROCESSORS%"
if not defined MAX_CORES set "MAX_CORES=1"
set "BUILD_CORES=4"
if %MAX_CORES% LSS 4 set "BUILD_CORES=%MAX_CORES%"
echo.
echo Building is CPU heavy; the first build compiles every dependency and can take several minutes.
echo More cores build faster but leave less CPU for other programs. 1 core is extremely slow.
set "CORES_INPUT="
set /p "CORES_INPUT=CPU cores to use for building [%BUILD_CORES%] (you have %MAX_CORES%): "
if not defined CORES_INPUT goto :coresok
REM Accept digits only; anything else keeps the default.
echo %CORES_INPUT%| findstr /r "^[0-9][0-9]*$" >nul || (
    echo Not a number, using %BUILD_CORES%.
    goto :coresok
)
REM Strip leading zeros (set /a would read them as octal) and treat huge values as "all".
for /f "tokens=* delims=0" %%N in ("%CORES_INPUT%") do set "CORES_INPUT=%%N"
if not defined CORES_INPUT set "CORES_INPUT=0"
if not "%CORES_INPUT:~4%"=="" set "CORES_INPUT=%MAX_CORES%"
set /a BUILD_CORES=CORES_INPUT
if %BUILD_CORES% GTR %MAX_CORES% set "BUILD_CORES=%MAX_CORES%"
if %BUILD_CORES% LSS 1 set "BUILD_CORES=1"
:coresok
set "CARGO_BUILD_JOBS=%BUILD_CORES%"

:build
echo Building sandbox (%PROFILE%) using %CARGO_BUILD_JOBS% core(s)...
cargo build -p sandbox %CARGO_FLAGS%
if errorlevel 1 goto :fail
goto :run

:nocargo
if not exist "LocalData\Build\%PROFILE%\sandbox.exe" (
    echo cargo was not found and there is no built sandbox. Install Rust from https://rustup.rs ^(the toolchain in rust-toolchain.toml is fetched automatically^).
    goto :fail
)
echo cargo was not found; starting the existing build.

:run
set "BEVY_ASSET_ROOT=%~dp0Sandbox"
start "RFG Sandbox" "LocalData\Build\%PROFILE%\sandbox.exe"
popd
exit /b 0

:fail
popd
pause
exit /b 1
