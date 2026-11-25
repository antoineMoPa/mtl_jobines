"""Import the authored Claydash ski set as metre-scale, +X-forward glTF geometry."""
import json
from pathlib import Path
import struct
import sys

source = Path(sys.argv[1]) if len(sys.argv) > 1 else Path('../model-harness/models/a-set-of-skis-and-sticks-a-ski-boots-low-poly-4/assembly.glb')
output = Path('data/equipment/ski-set/model.glb')
raw = source.read_bytes()
json_size = struct.unpack_from('<I', raw, 12)[0]
doc = json.loads(raw[20:20 + json_size])
binary = bytearray(raw[28 + json_size:])
seen = set()
for mesh in doc['meshes']:
    for primitive in mesh['primitives']:
        for attribute, accessor_id in primitive['attributes'].items():
            if attribute not in ('POSITION', 'NORMAL') or accessor_id in seen:
                continue
            seen.add(accessor_id)
            accessor = doc['accessors'][accessor_id]
            assert accessor['componentType'] == 5126 and accessor['type'] == 'VEC3'
            view = doc['bufferViews'][accessor['bufferView']]
            offset = view.get('byteOffset', 0) + accessor.get('byteOffset', 0)
            stride = view.get('byteStride', 12)
            points = []
            for i in range(accessor['count']):
                x, y, z = struct.unpack_from('<3f', binary, offset + i * stride)
                scale = .001 if attribute == 'POSITION' else 1.
                point = (z * scale, y * scale, -x * scale)
                struct.pack_into('<3f', binary, offset + i * stride, *point)
                points.append(point)
            if attribute == 'POSITION':
                accessor['min'] = [min(p[a] for p in points) for a in range(3)]
                accessor['max'] = [max(p[a] for p in points) for a in range(3)]
encoded = json.dumps(doc, separators=(',', ':')).encode()
encoded += b' ' * (-len(encoded) % 4)
binary += b'\0' * (-len(binary) % 4)
output.write_bytes(struct.pack('<III', 0x46546C67, 2, 28 + len(encoded) + len(binary)) + struct.pack('<II', len(encoded), 0x4E4F534A) + encoded + struct.pack('<II', len(binary), 0x004E4942) + binary)
print(f'{output}: {output.stat().st_size} bytes, {len(doc["nodes"])} authored parts')
