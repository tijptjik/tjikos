"""Select Telegram 7.3.3 theme dependencies without reading account keys.

Format references: Telegram/SourceFiles/storage/{localstorage.cpp,
storage_file_utilities.cpp,storage_settings_scheme.h} at upstream v7.3.3.
The global settings key derives from an empty passphrase and its stored salt.
Unknown records fail closed rather than widening the file selection.
"""
import hashlib
import hmac
import os
import stat
import struct


class TelegramSettingsError(ValueError):
    pass


class Reader:
    def __init__(self, data):
        self.data = data
        self.pos = 0

    def take(self, count):
        if count < 0 or count > len(self.data) - self.pos:
            raise TelegramSettingsError("Truncated Telegram settings/theme record")
        value = self.data[self.pos:self.pos + count]
        self.pos += count
        return value

    def number(self, fmt):
        return struct.unpack(fmt, self.take(struct.calcsize(fmt)))[0]

    def array(self):
        length = self.number(">I")
        return b"" if length == 0xFFFFFFFF else self.take(length)

    def string(self):
        try:
            return self.array().decode("utf-16-be")
        except UnicodeDecodeError as exc:
            raise TelegramSettingsError("Invalid Telegram theme string") from exc


def read_tdf(path, limit):
    # Do not follow links into account state or allow unbounded file reads.
    try:
        fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
        with os.fdopen(fd, "rb") as stream:
            info = os.fstat(stream.fileno())
            if not stat.S_ISREG(info.st_mode) or info.st_size > limit:
                raise TelegramSettingsError(f"Telegram settings/theme exceeds its {limit // (1024 * 1024)} MiB input limit")
            raw = stream.read(limit + 1)
    except OSError as exc:
        raise TelegramSettingsError("Missing or unsafe Telegram settings/theme dependency") from exc
    if len(raw) < 24 or len(raw) > limit or raw[:4] != b"TDF$":
        raise TelegramSettingsError("Invalid Telegram settings/theme file")
    data = raw[8:-16]
    checksum = hashlib.md5(
        data + struct.pack("<I", len(data)) + raw[4:8] + raw[:4],
        usedforsecurity=False,
    ).digest()
    if not hmac.compare_digest(checksum, raw[-16:]):
        raise TelegramSettingsError("Telegram settings/theme checksum mismatch")
    return data, hashlib.sha256(raw).hexdigest(), raw[4:8]


def aes_cipher():
    try:
        from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes
    except ImportError as exc:
        raise TelegramSettingsError(
            "Telegram theme selection requires Python cryptography (python3-cryptography)"
        ) from exc
    return Cipher, algorithms, modes


def aes_parameters(message_key, key):
    # Telegram's local storage uses the old MTProto KDF, receive direction x=8.
    a = hashlib.sha1(message_key + key[8:40]).digest()
    b = hashlib.sha1(key[40:56] + message_key + key[56:72]).digest()
    c = hashlib.sha1(key[72:104] + message_key).digest()
    d = hashlib.sha1(message_key + key[104:136]).digest()
    return a[:8] + b[8:20] + c[4:16], a[8:20] + b[:8] + c[16:20] + d[:8]


def decrypt_local(encrypted, key):
    Cipher, algorithms, modes = aes_cipher()
    if len(encrypted) <= 16 or len(encrypted) % 16:
        raise TelegramSettingsError("Invalid Telegram encrypted record length")
    message_key = encrypted[:16]
    aes_key, iv = aes_parameters(message_key, key)
    cipher = Cipher(algorithms.AES(aes_key), modes.ECB()).decryptor()
    previous_cipher, previous_plain = iv[:16], iv[16:]
    plain = bytearray()
    for offset in range(16, len(encrypted), 16):
        block = encrypted[offset:offset + 16]
        value = cipher.update(bytes(x ^ y for x, y in zip(block, previous_plain)))
        output = bytes(x ^ y for x, y in zip(value, previous_cipher))
        plain.extend(output)
        previous_cipher, previous_plain = block, output
    cipher.finalize()
    if not hmac.compare_digest(hashlib.sha1(plain).digest()[:16], message_key):
        raise TelegramSettingsError("Telegram settings/theme cannot be decrypted with global settings key")
    length = struct.unpack_from("<I", plain)[0]
    if not max(3, len(plain) - 16) < length <= len(plain):
        raise TelegramSettingsError("Invalid Telegram decrypted record length")
    return bytes(plain[4:length])


def encrypt_local(data, key):
    """Encode only a disposable recovery copy, deterministically for hashing.

    The outer age archive provides secrecy; this public-salt Telegram format
    is retained for compatibility, not used as the backup's encryption layer.
    """
    Cipher, algorithms, modes = aes_cipher()
    plain = struct.pack("<I", len(data) + 4) + data
    plain += bytes(-len(plain) % 16)
    message_key = hashlib.sha1(plain).digest()[:16]
    aes_key, iv = aes_parameters(message_key, key)
    cipher = Cipher(algorithms.AES(aes_key), modes.ECB()).encryptor()
    previous_cipher, previous_plain = iv[:16], iv[16:]
    encrypted = bytearray(message_key)
    for offset in range(0, len(plain), 16):
        block = plain[offset:offset + 16]
        value = cipher.update(bytes(x ^ y for x, y in zip(block, previous_cipher)))
        output = bytes(x ^ y for x, y in zip(value, previous_plain))
        encrypted.extend(output)
        previous_cipher, previous_plain = output, block
    cipher.finalize()
    return bytes(encrypted)


def encode_tdf(data, version):
    checksum = hashlib.md5(
        data + struct.pack("<I", len(data)) + version + b"TDF$",
        usedforsecurity=False,
    ).digest()
    return b"TDF$" + version + data + checksum


def read_settings(root):
    # Current Telegram writes the atomic 's' slot. Legacy 0/1 pairs depend on
    # mtimes and require migration in Telegram before enabling theme recovery.
    data, fingerprint, _ = read_tdf(root / "settingss", 1024 * 1024)
    outer = Reader(data)
    salt, encrypted = outer.array(), outer.array()
    if len(salt) != 32 or outer.pos != len(data):
        raise TelegramSettingsError("Unsupported Telegram global settings envelope")
    key = hashlib.pbkdf2_hmac("sha1", b"", salt, 4, 256)
    reader = Reader(decrypt_local(encrypted, key))
    themes = None
    while reader.pos < len(reader.data):
        record = reader.number(">I")
        if record in {0x06, 0x07, 0x1D, 0x0A, 0x0C, 0x0D, 0x58, 0x57}:
            reader.take(4)
        elif record in {0x60, 0x5E, 0x23}:
            reader.array()
        elif record == 0x54:
            if themes is not None:
                raise TelegramSettingsError("Duplicate Telegram theme settings")
            day, night = reader.number(">Q"), reader.number(">Q")
            mode = reader.number(">I")
            if mode not in (0, 1):
                raise TelegramSettingsError("Invalid Telegram night-mode value")
            themes = {"day": day, "night": night, "night_mode": bool(mode)}
        elif record in {0x4E, 0x5A}:
            reader.take(8)  # Language-pack keys; not theme dependencies.
        elif record == 0x61:
            reader.take(16)  # Separate chat backgrounds are outside this selection.
        elif record == 0x55:
            reader.take(8)
        else:
            raise TelegramSettingsError(
                f"Unsupported Telegram settings record {record:#x}; review format before backup"
            )
    if themes is None or (themes["night_mode"] and not themes["night"]):
        raise TelegramSettingsError("Missing Telegram theme selection")
    return themes, key, fingerprint


def compact_theme(data):
    reader = Reader(data)
    if len(reader.array()) > 5 * 1024 * 1024:
        raise TelegramSettingsError("Telegram theme content exceeds 5 MiB")
    if reader.string() != "special://new_tag":
        raise TelegramSettingsError("Unsupported Telegram theme format")
    reader.string()  # Absolute path; values are never logged.
    reader.string()  # Relative path.
    reader.take(16)  # Cloud theme ID and access hash, not account credentials.
    reader.string()  # Slug.
    reader.string()  # Title.
    reader.take(12)  # Document ID and creator field.
    cache_start = reader.pos
    reader.take(8)  # Palette and content checksums.
    reader.array()  # Compiled colors.
    reader.array()  # Theme background, bundled with the theme.
    cache_end = reader.pos
    reader.take(4)  # Tiling/creator field.
    if reader.pos < len(data):
        reader.array()  # Optional chat-theme metadata.
    if reader.pos != len(data):
        raise TelegramSettingsError("Unsupported trailing Telegram theme data")
    # Zero checksums and empty QByteArrays cause Telegram to regenerate caches.
    # Preserve the theme definition, identity, paths, tiling and chat metadata.
    return data[:cache_start] + bytes(16) + data[cache_end:]


def theme_recovery(root):
    """Return source hashes and cache-free theme bytes for the recovery copy."""
    themes, key, fingerprint = read_settings(root)
    required = {"settingss": fingerprint}
    compacted = {}
    for value in {themes["day"], themes["night"]} - {0}:
        name = f"{value:016X}"[::-1] + "s"
        data, fingerprint, version = read_tdf(root / name, 64 * 1024 * 1024)
        reader = Reader(data)
        encrypted = reader.array()
        if reader.pos != len(data):
            raise TelegramSettingsError("Unsupported Telegram theme envelope")
        content = compact_theme(decrypt_local(encrypted, key))
        compact_encrypted = encrypt_local(content, key)
        encoded = encode_tdf(struct.pack(">I", len(compact_encrypted)) + compact_encrypted, version)
        if len(encoded) > 8 * 1024 * 1024:
            raise TelegramSettingsError("Compacted Telegram theme exceeds 8 MiB")
        required[name] = fingerprint
        compacted[name] = encoded
    return required, compacted


def theme_dependencies(root):
    """Exact validated dependencies, without any account-key access."""
    return theme_recovery(root)[0]
