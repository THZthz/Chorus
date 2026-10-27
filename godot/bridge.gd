class_name ChorusBridge
extends RefCounted

var _pipe: Dictionary = {}
var _stdio: FileAccess = null
var _pid: int = -1

func start(repo_root: String, sw_rel_path: String = "docs/emberwatch.sw") -> Dictionary:
	stop()
	var sw_path := sw_rel_path if sw_rel_path.is_absolute_path() else repo_root.path_join(sw_rel_path)
	var is_win := OS.get_name() == "Windows"
	var exe_suffix := ".exe" if is_win else ""
	var release_bin := repo_root.path_join("target/release/story-cli" + exe_suffix)
	var debug_bin := repo_root.path_join("target/debug/story-cli" + exe_suffix)
	var bin := ""
	var args := PackedStringArray()

	var debug_mtime: int = FileAccess.get_modified_time(debug_bin) if FileAccess.file_exists(debug_bin) else 0
	var release_mtime: int = FileAccess.get_modified_time(release_bin) if FileAccess.file_exists(release_bin) else 0

	if debug_mtime > 0 or release_mtime > 0:
		bin = release_bin if release_mtime > debug_mtime else debug_bin
		args = PackedStringArray(["bridge", sw_path])
	else:
		bin = "cargo"
		args = PackedStringArray(["run", "--quiet", "--manifest-path", repo_root.path_join("Cargo.toml"), "-p", "story-cli", "--", "bridge", sw_path])

	_pipe = OS.execute_with_pipe(bin, args, false)
	if _pipe.is_empty():
		return {"ok": false, "error": "Failed to launch story-cli bridge"}

	_stdio = _pipe.get("stdio")
	_pid = _pipe.get("pid", -1)
	return _read_response()

func request(payload: Dictionary) -> Dictionary:
	if _stdio == null or not _stdio.is_open():
		return {"ok": false, "error": "Bridge pipe is not open"}
	_stdio.store_line(JSON.stringify(payload))
	_stdio.flush()
	return _read_response()

func _read_response() -> Dictionary:
	if _stdio == null:
		return {"ok": false, "error": "No stdio handle"}
	var attempts := 0
	while attempts < 500:
		var line := _stdio.get_line()
		if not line.is_empty():
			var parsed = JSON.parse_string(line)
			if typeof(parsed) == TYPE_DICTIONARY:
				return parsed
			return {"ok": false, "error": "Invalid JSON line: " + line}
		if not OS.is_process_running(_pid):
			break
		OS.delay_msec(10)
		attempts += 1
	return {"ok": false, "error": "Timed out waiting for story-cli bridge"}

func stop() -> void:
	if _stdio != null and _stdio.is_open():
		_stdio.store_line('{"cmd":"quit"}')
		_stdio.flush()
		_stdio = null
	if _pid > 0 and OS.is_process_running(_pid):
		OS.kill(_pid)
	_pid = -1
	_pipe.clear()
