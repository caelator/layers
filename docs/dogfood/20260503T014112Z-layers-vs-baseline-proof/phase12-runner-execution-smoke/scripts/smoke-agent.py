from pathlib import Path
import sys

prompt = sys.stdin.read()
Path('agent-output.txt').write_text(
    'phase12 smoke agent executed\n'
    f'prompt_bytes={len(prompt.encode("utf-8"))}\n'
    f'prompt_prefix={prompt[:120]!r}\n',
    encoding='utf-8',
)
print('phase12 smoke agent wrote agent-output.txt')
