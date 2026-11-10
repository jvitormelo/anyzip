// Thin C wrappers around the UnRAR DLL entry points so:
// 1. no C++ exception (`std::bad_alloc`, `std::length_error`, etc. — `RARReadHeaderEx` and
//    `RARCloseArchive` in upstream `dll.cpp` only catch `RAR_EXIT`) can ever unwind across
//    `extern "C"`;
// 2. `ErrHandler` is cleaned before each header/file operation (upstream `RAROpenArchiveEx` leaves
//    `RARX_WARNING` set on truncated QuickOpen blocks, which would otherwise make the first
//    `RARReadHeaderEx` fail with `ERAR_UNKNOWN`);
// plus layout probes for Rust tests.
#include <new>
#include "rar.hpp"

extern "C" {

void *anyzip_rar_open(RAROpenArchiveDataEx *data) {
  try {
    return RAROpenArchiveEx(data);
  } catch (const std::bad_alloc &) {
    if (data != nullptr) data->OpenResult = ERAR_NO_MEMORY;
    return nullptr;
  } catch (...) {
    if (data != nullptr && data->OpenResult == 0) data->OpenResult = ERAR_BAD_ARCHIVE;
    return nullptr;
  }
}

int anyzip_rar_close(void *handle) {
  try {
    ErrHandler.Clean();
    return RARCloseArchive(handle);
  } catch (const std::bad_alloc &) {
    return ERAR_NO_MEMORY;
  } catch (...) {
    return ERAR_ECLOSE;
  }
}

int anyzip_rar_read_header(void *handle, RARHeaderDataEx *data) {
  try {
    ErrHandler.Clean();
    return RARReadHeaderEx(handle, data);
  } catch (const std::bad_alloc &) {
    return ERAR_NO_MEMORY;
  } catch (...) {
    return ERAR_BAD_ARCHIVE;
  }
}

int anyzip_rar_process_file(void *handle, int op, wchar_t *dest_path, wchar_t *dest_name) {
  try {
    ErrHandler.Clean();
    return RARProcessFileW(handle, op, dest_path, dest_name);
  } catch (const std::bad_alloc &) {
    return ERAR_NO_MEMORY;
  } catch (...) {
    return ERAR_BAD_ARCHIVE;
  }
}

int anyzip_rar_dll_version() { return RARGetDllVersion(); }

size_t anyzip_sizeof_header_data_ex() { return sizeof(RARHeaderDataEx); }
size_t anyzip_sizeof_open_archive_data_ex() { return sizeof(RAROpenArchiveDataEx); }

}
