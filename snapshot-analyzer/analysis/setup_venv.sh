#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
VENV_DIR="${SCRIPT_DIR}/.venv"

python3 -m venv "${VENV_DIR}"
source "${VENV_DIR}/bin/activate"
python -m pip install --upgrade pip
python -m pip install -r "${SCRIPT_DIR}/requirements.txt"

echo ""
echo "Virtual environment is ready:"
echo "  source \"${VENV_DIR}/bin/activate\""
echo "Then run:"
echo "  python \"${SCRIPT_DIR}/analyze_output_csv.py\" --input \"${SCRIPT_DIR}/../../output.csv\""
