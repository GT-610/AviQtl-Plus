import re
import os

# Path definitions (matching project structure)
CSS_PATH = 'ui/resources/remixicon.css'
OUT_PATH = 'ui/qml/common/Icons.js'

if not os.path.exists(CSS_PATH):
    print(f"Error: {CSS_PATH} not found. Please place the file.")
    exit(1)

with open(CSS_PATH, 'r', encoding='utf-8') as f:
    css_content = f.read()

# Extract using regex
pattern = r'\.ri-([\w-]+):before\s*\{\s*content:\s*"\\(\w+)"'
matches = re.findall(pattern, css_content)

os.makedirs(os.path.dirname(OUT_PATH), exist_ok=True)
with open(OUT_PATH, 'w', encoding='utf-8') as f:
    f.write(".pragma library\n\nconst RI = {\n")
    for name, code in matches:
        js_name = name.replace('-', '_')
        f.write(f'    "{js_name}": "\\u{code}",\n')
    f.write("};\n")

print(f"Success: Exported {len(matches)} icons to {OUT_PATH}.")