@echo off
REM Extracts the assets the Bevy sandbox needs from YOUR copy of Red Faction Guerrilla (Steam, v1.0.2.1).
REM Nothing is downloaded except the Python packages numpy, Pillow and scipy (into LocalData\venv).
REM
REM Usage: ExtractAssets.bat ["C:\path\to\Red Faction Guerrilla"] [extract_all.py options]
REM   options: --force (redo finished steps)   --clean (delete LocalData\Extracted when done)
REM            --only STEP,STEP (misc,items,anims,player,chunks,assets,tables,build)
REM            --chunks NAME ... (build only those chunks, e.g. --chunks 0203house01_a)
REM Needs about 3 GB of free disk space and takes roughly an hour. It can be interrupted and re-run.
REM The game folder is remembered in LocalData\game_dir.txt.

setlocal EnableExtensions
pushd "%~dp0"

REM ---- arguments: an optional game folder first, everything else goes to extract_all.py ----
set "GAME="
set "PASSTHRU="
set "ARG1=%~1"
if not defined ARG1 goto :restargs
if "%ARG1:~0,2%"=="--" goto :restargs
set "GAME=%ARG1%"
shift
:restargs
if "%~1"=="" goto :argsdone
set PASSTHRU=%PASSTHRU% %1
shift
goto :restargs
:argsdone

REM ---- Python 3.9+ ----
set "PY="
py -3 -c "import sys; sys.exit(sys.version_info < (3, 9))" >nul 2>nul && set "PY=py -3"
if defined PY goto :havepy
python -c "import sys; sys.exit(sys.version_info < (3, 9))" >nul 2>nul && set "PY=python"
if defined PY goto :havepy
echo Python 3.9 or newer was not found. Install it from https://www.python.org/downloads/
echo and tick "Add python.exe to PATH", then run this again.
goto :fail
:havepy

REM ---- game folder: argument, remembered path, Steam, or Windows folder picker ----
if not defined GAME if exist "LocalData\game_dir.txt" set /p GAME=<"LocalData\game_dir.txt"
if defined GAME if not exist "%GAME%\RFG.exe" set "GAME="
if defined GAME if not exist "%GAME%\build\pc\cache\" set "GAME="
if defined GAME goto :gamefound
for /f "usebackq delims=" %%G in (`%PY% -I "Extractors\find_game.py"`) do set "GAME=%%G"
if defined GAME goto :gamefound
echo Could not find Red Faction Guerrilla automatically.
set "GAME="
for /f "usebackq delims=" %%G in (`powershell.exe -NoProfile -STA -ExecutionPolicy Bypass -File "Extractors\pick_folder.ps1"`) do set "GAME=%%G"
if not defined GAME goto :cancelled
:gamefound
if "%GAME:~-1%"=="\" set "GAME=%GAME:~0,-1%"
if not exist "LocalData" mkdir "LocalData"
>"LocalData\game_dir.txt" echo(%GAME%
echo Game folder: %GAME%

REM ---- virtual environment with the three packages ----
if exist "LocalData\venv\Scripts\python.exe" goto :havevenv
echo Creating LocalData\venv ...
%PY% -m venv "LocalData\venv"
if errorlevel 1 goto :fail
:havevenv
set "VPY=LocalData\venv\Scripts\python.exe"
"%VPY%" -c "import numpy, PIL, scipy" >nul 2>nul
if not errorlevel 1 goto :havepkgs
echo Installing numpy, Pillow and scipy ...
"%VPY%" -m pip install --disable-pip-version-check numpy pillow scipy
if errorlevel 1 goto :fail
:havepkgs

REM ---- extract ----
"%VPY%" "Extractors\extract_all.py" --game "%GAME%"%PASSTHRU%
if errorlevel 1 goto :fail
echo.
echo Extraction command completed. See the verification or selected-step summary above.
popd
if not defined RFG_NOPAUSE pause
exit /b 0

:cancelled
echo No game folder selected. Extraction stopped.
popd
if not defined RFG_NOPAUSE pause
exit /b 1

:fail
echo.
echo Extraction did not finish. Fix the problem above and run ExtractAssets.bat again; finished steps are skipped.
popd
if not defined RFG_NOPAUSE pause
exit /b 1
