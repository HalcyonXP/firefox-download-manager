"""Closed authority for ordinary unsigned-XPI UI observations; no install bypass."""
import hashlib
import io
import json
import zipfile

ADDON = "download-manager@halcyonxp.local"
PAYLOADS = {"background.js", "background.js.map", "manager.js", "manager.js.map",
            "manifest.json", "manager.html", "manager.css", "LICENSE.txt", "THIRD-PARTY-NOTICES.txt"}
LIMIT = 8 * 1024 * 1024
PERMISSIONS = {"nativeMessaging", "menus", "storage"}


def unique_object(pairs):
    value = {}
    for key, item in pairs:
        if key in value: raise RuntimeError("duplicate manifest member")
        value[key] = item
    return value


def inspect_xpi(data):
    """Current manual production manifest only; broader authority needs new review."""
    if not isinstance(data, bytes) or not 0 < len(data) <= LIMIT:
        raise RuntimeError("XPI size refused")
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        entries = archive.infolist()
        if len(entries) != len(PAYLOADS) or {item.filename for item in entries} != PAYLOADS:
            raise RuntimeError("XPI payload inventory refused")
        if any(item.flag_bits & 1 or (item.external_attr >> 16) & 0o170000 not in (0, 0o100000) or not 0 < item.file_size <= 2 * 1024 * 1024 for item in entries):
            raise RuntimeError("XPI payload bounds refused")
        if sum(item.file_size for item in entries) > LIMIT:
            raise RuntimeError("XPI expansion bound refused")
        manifest = json.loads(archive.read("manifest.json"), object_pairs_hook=unique_object)
        for item in entries: archive.read(item)  # Independently validate every member CRC.
    expected = {"manifest_version": 3, "name": "Firefox Download Manager", "version": "0.1.0",
                "permissions": ["nativeMessaging", "menus", "storage"], "optional_permissions": ["cookies"],
                "optional_host_permissions": ["http://*/*", "https://*/*"], "incognito": "not_allowed"}
    if not isinstance(manifest, dict): raise RuntimeError("manifest object required")
    if any(manifest.get(key) != value for key, value in expected.items()):
        raise RuntimeError("XPI permission or identity policy refused")
    if manifest.get("browser_specific_settings") != {"gecko": {"id": ADDON, "strict_min_version": "156.0"}}:
        raise RuntimeError("XPI browser identity refused")
    if set(manifest) != {*expected, "description", "browser_specific_settings", "background", "action", "content_security_policy"}:
        raise RuntimeError("unexpected XPI manifest authority")
    return {"xpi_sha256": hashlib.sha256(data).hexdigest(), "addon_id": ADDON,
            "name": expected["name"], "version": expected["version"]}


def approval(snapshot, identity):
    """Accept only two known visible UI steps for exactly one matching install.

    The caller clicks the actual enabled primary button, never its callback.
    Unknown prompts, security delays and missing permissions are not approval.
    """
    keys = {"id", "open", "enabled", "label", "source_matches", "install_count", "addon_id", "name", "permission_schema", "permissions", "origins"}
    if not isinstance(snapshot, dict) or set(snapshot) != keys:
        raise RuntimeError("install notification schema refused")
    if (snapshot["open"] is not True or snapshot["enabled"] is not True or snapshot["source_matches"] is not True
            or type(snapshot["install_count"]) is not int or snapshot["install_count"] != 1):
        raise RuntimeError("visible owned install authority unavailable")
    if snapshot["id"] == "addon-install-blocked":
        if snapshot["label"] != "Continue to Installation":
            raise RuntimeError("site installation warning changed")
        if snapshot["addon_id"] is not None and snapshot["addon_id"] != identity["addon_id"]:
            raise RuntimeError("foreign pending addon")
    elif snapshot["id"] == "addon-webext-permissions":
        if (snapshot["label"] != "Add" or snapshot["addon_id"] != identity["addon_id"]
                or snapshot["name"] != identity["name"] or snapshot["permission_schema"] is not True or not isinstance(snapshot["permissions"], list)
                or len(snapshot["permissions"]) != len(PERMISSIONS) or not all(isinstance(item, str) for item in snapshot["permissions"]) or set(snapshot["permissions"]) != PERMISSIONS
                or snapshot["origins"] != []):
            raise RuntimeError("extension permission warning changed")
    else:
        raise RuntimeError("unknown install prompt; no approval")
    return f"#{snapshot['id']}-notification .popup-notification-primary-button"


def persistent_receipt(receipt, identity):
    expected = {"id": identity["addon_id"], "version": identity["version"], "active": True,
                "temporary": False, "scope": 1, "private_allowed": False}
    if (not isinstance(receipt, dict) or set(receipt) != set(expected) or receipt != expected
            or type(receipt["scope"]) is not int
            or any(type(receipt[key]) is not bool for key in ("active", "temporary", "private_allowed"))):
        raise RuntimeError("persistent active addon receipt unavailable")
    return True
