"""Synthetic Telegram envelopes; no real account or settings data."""
import hashlib
import struct
from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes


def array(value):
    return struct.pack('>I', len(value)) + value


def string(value):
    return array(value.encode('utf-16-be'))


def encrypt(data, key):
    plain = struct.pack('<I', len(data) + 4) + data
    plain += bytes((-len(plain)) % 16)
    msg = hashlib.sha1(plain).digest()[:16]
    a = hashlib.sha1(msg + key[8:40]).digest()
    b = hashlib.sha1(key[40:56] + msg + key[56:72]).digest()
    c = hashlib.sha1(key[72:104] + msg).digest()
    d = hashlib.sha1(msg + key[104:136]).digest()
    cipher = Cipher(algorithms.AES(a[:8] + b[8:20] + c[4:16]), modes.ECB()).encryptor()
    previous_cipher = a[8:] + b[:4]
    previous_plain = b[4:8] + c[16:] + d[:8]
    output = bytearray(msg)
    for offset in range(0, len(plain), 16):
        block = plain[offset:offset + 16]
        value = cipher.update(bytes(x ^ y for x, y in zip(block, previous_cipher)))
        encrypted = bytes(x ^ y for x, y in zip(value, previous_plain))
        output.extend(encrypted)
        previous_cipher, previous_plain = encrypted, block
    cipher.finalize()
    return bytes(output)


def tdf(data):
    version = struct.pack('<I', 7003003)
    return b'TDF$' + version + data + hashlib.md5(
        data + struct.pack('<I', len(data)) + version + b'TDF$', usedforsecurity=False
    ).digest()


def write_fixture(root, night=True, theme_id=0x123456789ABCDEF0, extra=b'', theme_key=None, cached_background=b''):
    root.mkdir(parents=True, exist_ok=True)
    salt = bytes(range(32))
    key = hashlib.pbkdf2_hmac('sha1', b'', salt, 4, 256)
    theme = (array(b'windowBg: #101010;') + string('special://new_tag')
             + string('/synthetic/theme.tdesktop-palette') + string('')
             + bytes(16) + string('') + string('') + bytes(12)
             + bytes(8) + array(b'') + array(cached_background) + bytes(4) + array(b''))
    name = f'{theme_id:016X}'[::-1] + 's'
    (root / name).write_bytes(tdf(array(encrypt(theme, theme_key or key))))
    settings = (struct.pack('>II', 0x06, 1)
                + struct.pack('>I', 0x5E) + array(b'synthetic app settings')
                + struct.pack('>IQQI', 0x54, 0 if night else theme_id, theme_id if night else 0, int(night))
                + extra)
    (root / 'settingss').write_bytes(tdf(array(salt) + array(encrypt(settings, key))))
    return name
