#!/usr/bin/env python3

import argparse
import os
import shutil
import subprocess
import sys
import tarfile
import urllib.request
from pathlib import Path


VERSIONS = {
    "3.11": "3.11.13",
    "3.12": "3.12.10",
}


def run(command, *, cwd=None, env=None):
    print("+", " ".join(str(item) for item in command), flush=True)
    try:
        subprocess.run([str(item) for item in command], cwd=cwd, env=env, check=True)
    except subprocess.CalledProcessError as error:
        raise RuntimeError(
            f"Command failed with exit status {error.returncode}: "
            + " ".join(str(item) for item in command)
        ) from None


def download(version: str, archive: Path):
    url = f"https://www.python.org/ftp/python/{version}/{archive.name}"
    print(f"Downloading {url}", flush=True)
    urllib.request.urlretrieve(url, archive)


def extract(archive: Path, destination: Path):
    with tarfile.open(archive, "r:*") as source:
        source.extractall(destination, filter="data")


def patch_config_sub(source: Path):
    config_sub = source / "config.sub"
    text = config_sub.read_text()
    text = text.replace(
        "gnu* | android* | bsd* | mach* | minix* | genix*",
        "gnu* | android* | bsd* | mach* | minix* | genix* | octagram*",
    )
    needle = "linux-gnu* | linux-dietlibc* | linux-android* | linux-newlib* \\\n"
    replacement = (
        "linux-gnu* | linux-dietlibc* | linux-android* | linux-newlib* "
        "| octagram* \\\n"
    )
    if replacement not in text:
        text = text.replace(needle, replacement)
        config_sub.write_text(text)


def patch_cpython_platform(source: Path):
    for name in ("configure", "configure.ac"):
        path = source / name
        text = path.read_text()
        text = text.replace(
            "wasm32-*-* | wasm64-*-*)",
            "x86_64-*-octagram*)\n"
            "\t_host_cpu=$host_cpu\n"
            "\tMACHDEP=octagram\n"
            "\t;;\n"
            "\t    wasm32-*-* | wasm64-*-*)",
        )
        needle = "\t*-*-emscripten)"
        replacement = (
            "\t*-*-octagram)\n"
            "\t    ac_sys_system=OCTAGRAM\n"
            "\t    MACHDEP=octagram\n"
            "\t    _host_cpu=$host_cpu\n"
            "\t    ;;\n"
            "\t*-*-emscripten)"
        )
        if replacement not in text:
            text = text.replace(needle, replacement)
        path.write_text(text)


def patch_octagram_pyconfig(build: Path):
    path = build / "pyconfig.h"
    if not path.exists():
        return
    text = path.read_text()
    disabled = (
        "HAVE_CLOSE_RANGE",
        "HAVE_COPY_FILE_RANGE",
        "HAVE_DLOPEN",
        "HAVE_FACCESSAT",
        "HAVE_FCHMODAT",
        "HAVE_FCHOWN",
        "HAVE_FCHOWNAT",
        "HAVE_FDOPENDIR",
        "HAVE_FEXECVE",
        "HAVE_FUTIMENS",
        "HAVE_FUTIMES",
        "HAVE_GETGROUPS",
        "HAVE_GETGROUPLIST",
        "HAVE_GETLOGIN",
        "HAVE_GETLOADAVG",
        "HAVE_GETPGID",
        "HAVE_GETPGRP",
        "HAVE_GETRESGID",
        "HAVE_GETRESUID",
        "HAVE_GETSID",
        "HAVE_LOCKF",
        "HAVE_MKFIFO",
        "HAVE_MKFIFOAT",
        "HAVE_MKNOD",
        "HAVE_MKNODAT",
        "HAVE_POSIX_FADVISE",
        "HAVE_POSIX_FALLOCATE",
        "HAVE_POSIX_SPAWN",
        "HAVE_PREAD",
        "HAVE_PREADV",
        "HAVE_PWRITE",
        "HAVE_PWRITEV",
        "HAVE_PTHREAD_DESTRUCTOR",
        "HAVE_PTHREAD_GETCPUCLOCKID",
        "HAVE_PTHREAD_KILL",
        "HAVE_PTHREAD_SIGMASK",
        "HAVE_SIGALTSTACK",
        "HAVE_SPLICE",
        "HAVE_SYNC",
        "HAVE_TRUNCATE",
        "HAVE_TTYNAME_R",
        "HAVE_UTIMENSAT",
        "HAVE_WAIT3",
        "HAVE_WAIT4",
        "HAVE_WRITEV",
    )
    for name in disabled:
        text = text.replace(f"#define {name} 1", f"/* #undef {name} */")
    path.write_text(text)


def install_octagram_stubs(source: Path):
    posix = source / "Modules" / "posixmodule.c"
    posix.write_text(
        r'''#include "Python.h"
#include <unistd.h>
#include <sys/utsname.h>
        #include <sys/stat.h>
        #include <dirent.h>
        #include <string.h>
        #include <fcntl.h>
        #include <stddef.h>

static PyObject *
oct_getpid(PyObject *module, PyObject *args)
{
    (void)module;
    if (!PyArg_ParseTuple(args, ":getpid")) {
        return NULL;
    }
    return PyLong_FromLong((long)getpid());
}

static PyObject *
oct_getuid(PyObject *module, PyObject *args)
{
    (void)module;
    if (!PyArg_ParseTuple(args, ":getuid")) {
        return NULL;
    }
    return PyLong_FromLong(0);
}

static PyObject *
oct_getgid(PyObject *module, PyObject *args)
{
    (void)module;
    if (!PyArg_ParseTuple(args, ":getgid")) {
        return NULL;
    }
    return PyLong_FromLong(0);
}

static PyObject *
oct_getcwd(PyObject *module, PyObject *args)
{
    (void)module;
    char buffer[4096];
    if (!PyArg_ParseTuple(args, ":getcwd")) {
        return NULL;
    }
    if (getcwd(buffer, sizeof(buffer)) == NULL) {
        return PyErr_SetFromErrno(PyExc_OSError);
    }
    return PyUnicode_FromString(buffer);
}

static PyObject *
oct_chdir(PyObject *module, PyObject *args)
{
    (void)module;
    const char *path;
    if (!PyArg_ParseTuple(args, "s:chdir", &path)) {
        return NULL;
    }
    if (chdir(path) != 0) {
        return PyErr_SetFromErrno(PyExc_OSError);
    }
    Py_RETURN_NONE;
}

static PyObject *
oct_uname(PyObject *module, PyObject *args)
{
    (void)module;
    if (!PyArg_ParseTuple(args, ":uname")) {
        return NULL;
    }
    struct utsname name;
    if (uname(&name) != 0) {
        return PyErr_SetFromErrno(PyExc_OSError);
    }
    return Py_BuildValue("sssss", name.sysname, name.nodename,
                         name.release, name.version, name.machine);
}

static PyStructSequence_Field octagram_stat_result_fields[] = {
    {"st_mode", "protection bits"},
    {"st_ino", "inode"},
    {"st_dev", "device"},
    {"st_nlink", "link count"},
    {"st_uid", "user id"},
    {"st_gid", "group id"},
    {"st_size", "size in bytes"},
    {"st_atime", "time of last access"},
    {"st_mtime", "time of last modification"},
    {"st_ctime", "time of last status change"},
    {NULL, NULL}
};

static PyStructSequence_Desc octagram_stat_result_desc = {
    "os.stat_result",
    "OCTAGRAM file status",
    octagram_stat_result_fields,
    10
};

static PyTypeObject *octagram_stat_result_type;

static PyObject *
oct_stat(PyObject *module, PyObject *args)
{
    (void)module;
    const char *path;
    if (!PyArg_ParseTuple(args, "s:stat", &path)) {
        return NULL;
    }
    struct stat result;
    if (stat(path, &result) != 0) {
        return PyErr_SetFromErrnoWithFilename(PyExc_OSError, path);
    }
    PyObject *stat_result = PyStructSequence_New(octagram_stat_result_type);
    if (stat_result == NULL) {
        return NULL;
    }
    PyStructSequence_SET_ITEM(stat_result, 0, PyLong_FromUnsignedLong(result.st_mode));
    PyStructSequence_SET_ITEM(stat_result, 1, PyLong_FromUnsignedLongLong(result.st_ino));
    PyStructSequence_SET_ITEM(stat_result, 2, PyLong_FromUnsignedLongLong(result.st_dev));
    PyStructSequence_SET_ITEM(stat_result, 3, PyLong_FromUnsignedLong(result.st_nlink));
    PyStructSequence_SET_ITEM(stat_result, 4, PyLong_FromUnsignedLong(result.st_uid));
    PyStructSequence_SET_ITEM(stat_result, 5, PyLong_FromUnsignedLong(result.st_gid));
    PyStructSequence_SET_ITEM(stat_result, 6, PyLong_FromLongLong(result.st_size));
    PyStructSequence_SET_ITEM(stat_result, 7, PyLong_FromLongLong(result.st_atime));
    PyStructSequence_SET_ITEM(stat_result, 8, PyLong_FromLongLong(result.st_mtime));
    PyStructSequence_SET_ITEM(stat_result, 9, PyLong_FromLongLong(result.st_ctime));
    return stat_result;
}

static PyObject *
oct_mkdir(PyObject *module, PyObject *args)
{
    (void)module;
    const char *path;
    unsigned int mode = 0777;
    if (!PyArg_ParseTuple(args, "s|I:mkdir", &path, &mode)) {
        return NULL;
    }
    if (mkdir(path, (mode_t)mode) != 0) {
        return PyErr_SetFromErrnoWithFilename(PyExc_OSError, path);
    }
    Py_RETURN_NONE;
}

static PyObject *
oct_unlink(PyObject *module, PyObject *args)
{
    (void)module;
    const char *path;
    if (!PyArg_ParseTuple(args, "s:unlink", &path)) {
        return NULL;
    }
    if (unlink(path) != 0) {
        return PyErr_SetFromErrnoWithFilename(PyExc_OSError, path);
    }
    Py_RETURN_NONE;
}

static PyObject *
oct_rmdir(PyObject *module, PyObject *args)
{
    (void)module;
    const char *path;
    if (!PyArg_ParseTuple(args, "s:rmdir", &path)) {
        return NULL;
    }
    if (rmdir(path) != 0) {
        return PyErr_SetFromErrnoWithFilename(PyExc_OSError, path);
    }
    Py_RETURN_NONE;
}

static PyObject *
oct_open(PyObject *module, PyObject *args)
{
    (void)module;
    const char *path;
    int flags;
    unsigned int mode = 0666;
    if (!PyArg_ParseTuple(args, "si|I:open", &path, &flags, &mode)) {
        return NULL;
    }
    int fd = open(path, flags, (mode_t)mode);
    if (fd < 0) {
        return PyErr_SetFromErrnoWithFilename(PyExc_OSError, path);
    }
    return PyLong_FromLong(fd);
}

static PyObject *
oct_close(PyObject *module, PyObject *args)
{
    (void)module;
    int fd;
    if (!PyArg_ParseTuple(args, "i:close", &fd)) {
        return NULL;
    }
    if (close(fd) != 0) {
        return PyErr_SetFromErrno(PyExc_OSError);
    }
    Py_RETURN_NONE;
}

static PyObject *
oct_read(PyObject *module, PyObject *args)
{
    (void)module;
    int fd;
    Py_ssize_t length;
    if (!PyArg_ParseTuple(args, "in:read", &fd, &length)) {
        return NULL;
    }
    if (length < 0) {
        PyErr_SetString(PyExc_ValueError, "negative read length");
        return NULL;
    }
    PyObject *bytes = PyBytes_FromStringAndSize(NULL, length);
    if (bytes == NULL) {
        return NULL;
    }
    ssize_t result = read(fd, PyBytes_AS_STRING(bytes), (size_t)length);
    if (result < 0) {
        Py_DECREF(bytes);
        return PyErr_SetFromErrno(PyExc_OSError);
    }
    if (_PyBytes_Resize(&bytes, result) < 0) {
        return NULL;
    }
    return bytes;
}

static PyObject *
oct_write(PyObject *module, PyObject *args)
{
    (void)module;
    int fd;
    Py_buffer buffer;
    if (!PyArg_ParseTuple(args, "iy*:write", &fd, &buffer)) {
        return NULL;
    }
    ssize_t result = write(fd, buffer.buf, (size_t)buffer.len);
    PyBuffer_Release(&buffer);
    if (result < 0) {
        return PyErr_SetFromErrno(PyExc_OSError);
    }
    return PyLong_FromSsize_t(result);
}

static PyObject *oct_rename(PyObject *module, PyObject *args);

static PyObject *
oct_replace(PyObject *module, PyObject *args)
{
    return oct_rename(module, args);
}

static PyObject *
oct_rename(PyObject *module, PyObject *args)
{
    (void)module;
    const char *source;
    const char *target;
    if (!PyArg_ParseTuple(args, "ss:rename", &source, &target)) {
        return NULL;
    }
    if (rename(source, target) != 0) {
        return PyErr_SetFromErrnoWithFilename(PyExc_OSError, source);
    }
    Py_RETURN_NONE;
}

static PyObject *
oct_fspath(PyObject *module, PyObject *args)
{
    (void)module;
    PyObject *path;
    if (!PyArg_ParseTuple(args, "O:fspath", &path)) {
        return NULL;
    }
    return PyOS_FSPath(path);
}

typedef struct {
    PyObject_HEAD
    PyObject *name;
    PyObject *path;
    int is_directory;
} OctagramDirEntry;

static void
octagram_dir_entry_dealloc(OctagramDirEntry *entry)
{
    Py_XDECREF(entry->name);
    Py_XDECREF(entry->path);
    Py_TYPE(entry)->tp_free((PyObject *)entry);
}

static PyObject *
octagram_dir_entry_is_dir(OctagramDirEntry *entry, PyObject *args)
{
    (void)args;
    if (entry->is_directory) {
        Py_RETURN_TRUE;
    }
    Py_RETURN_FALSE;
}

static PyObject *
octagram_dir_entry_is_symlink(OctagramDirEntry *entry, PyObject *args)
{
    (void)args;
    Py_RETURN_FALSE;
}

static PyObject *
octagram_dir_entry_stat(OctagramDirEntry *entry, PyObject *args)
{
    (void)args;
    PyObject *arguments = Py_BuildValue("(s)", PyUnicode_AsUTF8(entry->path));
    if (arguments == NULL) {
        return NULL;
    }
    PyObject *result = oct_stat(NULL, arguments);
    Py_DECREF(arguments);
    return result;
}

static PyMethodDef octagram_dir_entry_methods[] = {
    {"is_dir", (PyCFunction)octagram_dir_entry_is_dir, METH_VARARGS, "Check directory"},
    {"is_symlink", (PyCFunction)octagram_dir_entry_is_symlink, METH_VARARGS, "Check symlink"},
    {"stat", (PyCFunction)octagram_dir_entry_stat, METH_VARARGS, "Return entry status"},
    {NULL, NULL, 0, NULL}
};

static PyMemberDef octagram_dir_entry_members[] = {
    {"name", offsetof(OctagramDirEntry, name), Py_T_OBJECT_EX, Py_READONLY, "entry name"},
    {"path", offsetof(OctagramDirEntry, path), Py_T_OBJECT_EX, Py_READONLY, "entry path"},
    {NULL, 0, 0, 0, NULL}
};

static PyTypeObject octagram_dir_entry_type = {
    PyVarObject_HEAD_INIT(NULL, 0)
    .tp_name = "posix.DirEntry",
    .tp_basicsize = sizeof(OctagramDirEntry),
    .tp_dealloc = (destructor)octagram_dir_entry_dealloc,
    .tp_flags = Py_TPFLAGS_DEFAULT,
    .tp_doc = "OCTAGRAM directory entry",
    .tp_methods = octagram_dir_entry_methods,
    .tp_members = octagram_dir_entry_members,
};

static PyObject *
oct_scandir(PyObject *module, PyObject *args)
{
    (void)module;
    const char *path = ".";
    if (!PyArg_ParseTuple(args, "|s:scandir", &path)) {
        return NULL;
    }
    DIR *directory = opendir(path);
    if (directory == NULL) {
        return PyErr_SetFromErrnoWithFilename(PyExc_OSError, path);
    }
    PyObject *entries = PyList_New(0);
    if (entries == NULL) {
        closedir(directory);
        return NULL;
    }
    struct dirent *entry;
    while ((entry = readdir(directory)) != NULL) {
        if (strcmp(entry->d_name, ".") == 0 || strcmp(entry->d_name, "..") == 0) {
            continue;
        }
        OctagramDirEntry *item = PyObject_New(OctagramDirEntry, &octagram_dir_entry_type);
        if (item == NULL) {
            Py_DECREF(entries);
            closedir(directory);
            return NULL;
        }
        item->name = PyUnicode_FromString(entry->d_name);
        if (strcmp(path, ".") == 0) {
            item->path = PyUnicode_FromString(entry->d_name);
        } else {
            item->path = PyUnicode_FromFormat("%s/%s", path, entry->d_name);
        }
        item->is_directory = entry->d_type == DT_DIR;
        if (item->name == NULL || item->path == NULL || PyList_Append(entries, (PyObject *)item) != 0) {
            Py_DECREF(item);
            Py_DECREF(entries);
            closedir(directory);
            return NULL;
        }
        Py_DECREF(item);
    }
    closedir(directory);
    return PyObject_GetIter(entries);
}

static PyObject *
oct_listdir(PyObject *module, PyObject *args)
{
    (void)module;
    const char *path = ".";
    if (!PyArg_ParseTuple(args, "|s:listdir", &path)) {
        return NULL;
    }
    DIR *directory = opendir(path);
    if (directory == NULL) {
        return PyErr_SetFromErrnoWithFilename(PyExc_OSError, path);
    }
    PyObject *names = PyList_New(0);
    if (names == NULL) {
        closedir(directory);
        return NULL;
    }
    struct dirent *entry;
    while ((entry = readdir(directory)) != NULL) {
        if (strcmp(entry->d_name, ".") == 0 || strcmp(entry->d_name, "..") == 0) {
            continue;
        }
        PyObject *name = PyUnicode_FromString(entry->d_name);
        if (name == NULL || PyList_Append(names, name) != 0) {
            Py_XDECREF(name);
            Py_DECREF(names);
            closedir(directory);
            return NULL;
        }
        Py_DECREF(name);
    }
    closedir(directory);
    return names;
}

static PyObject *
oct_system(PyObject *module, PyObject *args)
{
    (void)module;
    const char *command;
    if (!PyArg_ParseTuple(args, "s:system", &command)) {
        return NULL;
    }
    (void)command;
    PyErr_SetString(PyExc_NotImplementedError,
                    "process execution is not implemented by OCTAGRAM");
    return NULL;
}

static PyMethodDef octagram_posix_methods[] = {
    {"getpid", oct_getpid, METH_VARARGS, "Return the current process ID"},
    {"getuid", oct_getuid, METH_VARARGS, "Return OCTAGRAM uid"},
    {"geteuid", oct_getuid, METH_VARARGS, "Return OCTAGRAM effective uid"},
    {"getgid", oct_getgid, METH_VARARGS, "Return OCTAGRAM gid"},
    {"getegid", oct_getgid, METH_VARARGS, "Return OCTAGRAM effective gid"},
    {"getcwd", oct_getcwd, METH_VARARGS, "Return the current working directory"},
    {"chdir", oct_chdir, METH_VARARGS, "Change the current working directory"},
    {"uname", oct_uname, METH_VARARGS, "Return OCTAGRAM uname data"},
    {"stat", oct_stat, METH_VARARGS, "Return OCTAGRAM file status"},
    {"listdir", oct_listdir, METH_VARARGS, "List an OCTAGRAM directory"},
    {"scandir", oct_scandir, METH_VARARGS, "Scan an OCTAGRAM directory"},
    {"fspath", oct_fspath, METH_VARARGS, "Return an OS filesystem path"},
    {"mkdir", oct_mkdir, METH_VARARGS, "Create an OCTAGRAM directory"},
    {"unlink", oct_unlink, METH_VARARGS, "Remove an OCTAGRAM file"},
    {"rmdir", oct_rmdir, METH_VARARGS, "Remove an OCTAGRAM directory"},
    {"rename", oct_rename, METH_VARARGS, "Rename an OCTAGRAM file"},
    {"replace", oct_replace, METH_VARARGS, "Replace an OCTAGRAM file"},
    {"open", oct_open, METH_VARARGS, "Open an OCTAGRAM file"},
    {"close", oct_close, METH_VARARGS, "Close an OCTAGRAM file descriptor"},
    {"read", oct_read, METH_VARARGS, "Read from an OCTAGRAM file descriptor"},
    {"write", oct_write, METH_VARARGS, "Write to an OCTAGRAM file descriptor"},
    {"system", oct_system, METH_VARARGS, "Execute a shell command"},
    {NULL, NULL, 0, NULL}
};

static struct PyModuleDef octagram_posix_module = {
    PyModuleDef_HEAD_INIT,
    "posix",
    "OCTAGRAM minimal POSIX interface",
    -1,
    octagram_posix_methods,
};

PyMODINIT_FUNC
PyInit_posix(void)
{
    if (PyType_Ready(&octagram_dir_entry_type) < 0) {
        return NULL;
    }
    PyObject *module = PyModule_Create(&octagram_posix_module);
    if (module == NULL) {
        return NULL;
    }
    octagram_stat_result_type = PyStructSequence_NewType(&octagram_stat_result_desc);
    if (octagram_stat_result_type == NULL) {
        Py_DECREF(module);
        return NULL;
    }
    if (PyModule_AddObjectRef(module, "stat_result",
                              (PyObject *)octagram_stat_result_type) < 0) {
        Py_DECREF((PyObject *)octagram_stat_result_type);
        Py_DECREF(module);
        return NULL;
    }
    PyModule_AddIntConstant(module, "O_RDONLY", O_RDONLY);
    PyModule_AddIntConstant(module, "O_WRONLY", O_WRONLY);
    PyModule_AddIntConstant(module, "O_RDWR", O_RDWR);
    PyModule_AddIntConstant(module, "O_CREAT", O_CREAT);
    PyModule_AddIntConstant(module, "O_TRUNC", O_TRUNC);
    PyModule_AddIntConstant(module, "O_APPEND", O_APPEND);
    PyModule_AddIntConstant(module, "O_EXCL", O_EXCL);
    PyModule_AddIntConstant(module, "O_CLOEXEC", O_CLOEXEC);
    PyModule_AddIntConstant(module, "O_NOFOLLOW", O_NOFOLLOW);
    PyObject *have_functions = Py_BuildValue("()");
    if (have_functions != NULL) {
        if (PyModule_AddObject(module, "_have_functions", have_functions) < 0) {
            Py_DECREF(have_functions);
        }
    }
    PyObject *environment = PyDict_New();
    if (environment != NULL) {
        if (PyModule_AddObject(module, "environ", environment) < 0) {
            Py_DECREF(environment);
        }
    }
    return module;
}
'''
    )

    signal = source / "Modules" / "signalmodule.c"
    signal.write_text(
        r'''#include "Python.h"
#include <signal.h>

static PyObject *
oct_signal(PyObject *module, PyObject *args)
{
    (void)module;
    int signal_number;
    PyObject *handler;
    if (!PyArg_ParseTuple(args, "iO:signal", &signal_number, &handler)) {
        return NULL;
    }
    (void)signal_number;
    (void)handler;
    PyErr_SetString(PyExc_NotImplementedError,
                    "Python signal handlers are not implemented by OCTAGRAM");
    return NULL;
}

static PyObject *
oct_getsignal(PyObject *module, PyObject *args)
{
    (void)module;
    int signal_number;
    if (!PyArg_ParseTuple(args, "i:getsignal", &signal_number)) {
        return NULL;
    }
    (void)signal_number;
    Py_RETURN_NONE;
}

static PyMethodDef octagram_signal_methods[] = {
    {"signal", oct_signal, METH_VARARGS, "Install a signal handler"},
    {"getsignal", oct_getsignal, METH_VARARGS, "Return the signal handler"},
    {NULL, NULL, 0, NULL}
};

static struct PyModuleDef octagram_signal_module = {
    PyModuleDef_HEAD_INIT,
    "signal",
    "OCTAGRAM minimal signal interface",
    -1,
    octagram_signal_methods,
};

PyMODINIT_FUNC
PyInit_signal(void)
{
    return PyModule_Create(&octagram_signal_module);
}

PyMODINIT_FUNC
PyInit__signal(void)
{
    return PyModule_Create(&octagram_signal_module);
}

int _PySignal_Init(int install_signal_handlers)
{
    (void)install_signal_handlers;
    return 0;
}

void _PySignal_Fini(void)
{
}

int PyErr_CheckSignals(void)
{
    return 0;
}

int _PyErr_CheckSignalsTstate(PyThreadState *tstate)
{
    (void)tstate;
    return 0;
}

int PyErr_SetInterruptEx(int signal_number)
{
    (void)signal_number;
    return 0;
}

int PyOS_InterruptOccurred(void)
{
    return 0;
}

int _PyOS_InterruptOccurred(void)
{
    return 0;
}

PyObject *PyOS_FSPath(PyObject *object)
{
    if (PyUnicode_Check(object) || PyBytes_Check(object)) {
        return Py_NewRef(object);
    }
    PyObject *method = PyObject_GetAttrString(object, "__fspath__");
    if (method == NULL) {
        PyErr_Clear();
        PyErr_Format(PyExc_TypeError, "expected str, bytes or os.PathLike object");
        return NULL;
    }
    PyObject *path = PyObject_CallNoArgs(method);
    Py_DECREF(method);
    if (path != NULL && !PyUnicode_Check(path) && !PyBytes_Check(path)) {
        PyErr_SetString(PyExc_TypeError, "__fspath__ returned non-str or non-byte");
        Py_DECREF(path);
        return NULL;
    }
    return path;
}
'''
    )

    (source / "Modules" / "pwdmodule.c").write_text(
        r'''#include "Python.h"

static PyStructSequence_Field octagram_passwd_fields[] = {
    {"pw_name", "user name"},
    {"pw_passwd", "password"},
    {"pw_uid", "user id"},
    {"pw_gid", "group id"},
    {"pw_gecos", "real name"},
    {"pw_dir", "home directory"},
    {"pw_shell", "shell"},
    {NULL, NULL}
};

static PyStructSequence_Desc octagram_passwd_desc = {
    "pwd.struct_passwd",
    "OCTAGRAM password record",
    octagram_passwd_fields,
    7
};

static PyTypeObject *octagram_passwd_type;

static PyObject *octagram_get_passwd(void)
{
    PyObject *result = PyStructSequence_New(octagram_passwd_type);
    if (result == NULL) {
        return NULL;
    }
    PyStructSequence_SET_ITEM(result, 0, PyUnicode_FromString("octagram"));
    PyStructSequence_SET_ITEM(result, 1, PyUnicode_FromString("octagram"));
    PyStructSequence_SET_ITEM(result, 2, PyLong_FromLong(0));
    PyStructSequence_SET_ITEM(result, 3, PyLong_FromLong(0));
    PyStructSequence_SET_ITEM(result, 4, PyUnicode_FromString(""));
    PyStructSequence_SET_ITEM(result, 5, PyUnicode_FromString("/"));
    PyStructSequence_SET_ITEM(result, 6, PyUnicode_FromString("/bin/sh"));
    return result;
}

static PyObject *
oct_getpwuid(PyObject *module, PyObject *args)
{
    (void)module;
    unsigned int uid;
    if (!PyArg_ParseTuple(args, "I:getpwuid", &uid)) {
        return NULL;
    }
    if (uid != 0) {
        return PyErr_Format(PyExc_KeyError, "getpwuid: uid not found: %u", uid);
    }
    return octagram_get_passwd();
}

static PyObject *
oct_getpwnam(PyObject *module, PyObject *args)
{
    (void)module;
    const char *name;
    if (!PyArg_ParseTuple(args, "s:getpwnam", &name)) {
        return NULL;
    }
    if (name == NULL || name[0] == '\0') {
        return PyErr_SetFromErrno(PyExc_OSError);
    }
    if (strcmp(name, "octagram") != 0 && strcmp(name, "root") != 0) {
        return PyErr_Format(PyExc_KeyError, "getpwnam(): name not found: %s", name);
    }
    return octagram_get_passwd();
}

static PyMethodDef octagram_pwd_methods[] = {
    {"getpwuid", oct_getpwuid, METH_VARARGS, "Return the OCTAGRAM current user"},
    {"getpwnam", oct_getpwnam, METH_VARARGS, "Return an OCTAGRAM named user"},
    {NULL, NULL, 0, NULL}
};

static struct PyModuleDef octagram_pwd_module = {
    PyModuleDef_HEAD_INIT,
    "pwd",
    "OCTAGRAM minimal pwd interface",
    -1,
    octagram_pwd_methods,
};

PyMODINIT_FUNC
PyInit_pwd(void)
{
    PyObject *module = PyModule_Create(&octagram_pwd_module);
    if (module == NULL) {
        return NULL;
    }
    octagram_passwd_type = PyStructSequence_NewType(&octagram_passwd_desc);
    if (octagram_passwd_type == NULL) {
        Py_DECREF(module);
        return NULL;
    }
    if (PyModule_AddObjectRef(module, "struct_passwd",
                              (PyObject *)octagram_passwd_type) < 0) {
        Py_DECREF((PyObject *)octagram_passwd_type);
        Py_DECREF(module);
        return NULL;
    }
    return module;
}
'''
    )


def build_host_python(source: Path, work: Path, jobs: str) -> Path:
    host_build = work / "host-build"
    if (host_build / "python").exists():
        return host_build / "python"

    host_build.mkdir(parents=True, exist_ok=True)
    run([source / "configure", "--prefix=" + str(host_build / "install"),
         "--disable-test-modules", "--without-ensurepip"], cwd=host_build)
    run(["make", f"-j{jobs}"], cwd=host_build)
    run(["make", "install"], cwd=host_build)
    return host_build / "install" / "bin" / f"python{source.name.split('-')[1][:4]}"


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Prepare CPython 3.11/3.12 for the OCTAGRAM x86_64 ABI"
    )
    parser.add_argument("branch", choices=("3.11", "3.12", "both"))
    parser.add_argument(
        "--build",
        action="store_true",
        help="Configure and build CPython after downloading the source",
    )
    args = parser.parse_args()

    repo = Path(__file__).resolve().parents[1]
    user = repo / "user"
    work = repo / "build" / "cpython"
    work.mkdir(parents=True, exist_ok=True)

    branches = ("3.11", "3.12") if args.branch == "both" else (args.branch,)
    for branch in branches:
        version = VERSIONS[branch]
        archive = work / f"Python-{version}.tgz"
        if not archive.exists():
            download(version, archive)

        source = work / f"Python-{version}"
        if source.exists():
            shutil.rmtree(source)
        extract(archive, work)
        patch_config_sub(source)
        patch_cpython_platform(source)
        install_octagram_stubs(source)

        if not args.build:
            print(f"Prepared CPython {version} source at {source}")
            continue

        build = work / f"{branch}-octagram"
        build.mkdir(parents=True, exist_ok=True)
        host_python = build_host_python(source, build, str(os.cpu_count() or 2))

        env = dict(__import__("os").environ)
        env.update(
            {
                "CC": str(shutil.which("gcc") or "gcc"),
                "AR": str(shutil.which("ar") or "ar"),
                "RANLIB": str(shutil.which("ranlib") or "ranlib"),
                "CFLAGS": (
                    "-std=c11 -O2 -ffreestanding -fno-stack-protector "
                    "-fno-pie -fno-pic -mno-red-zone -mcmodel=small "
                    "-D_POSIX_THREADS=200809L "
                    f"-I{user / 'include'} -DOCTAGRAM=1"
                ),
            }
        )
        config_site = work / f"{branch}-octagram.config.site"
        config_site.write_text(
            "\n".join(
                (
                    "ac_cv_file__dev_ptmx=no",
                    "ac_cv_file__dev_ptc=no",
                    "ac_cv_func_dlopen=no",
                    "ac_cv_func_pthread_create=no",
                    "ac_cv_func_close_range=no",
                    "ac_cv_func_copy_file_range=no",
                    "ac_cv_func_faccessat=no",
                    "ac_cv_func_fchmodat=no",
                    "ac_cv_func_fchown=no",
                    "ac_cv_func_fchownat=no",
                    "ac_cv_func_fdopendir=no",
                    "ac_cv_func_futimens=no",
                    "ac_cv_func_futimes=no",
                    "ac_cv_func_getgroups=no",
                    "ac_cv_func_getgrouplist=no",
                    "ac_cv_func_getlogin=no",
                    "ac_cv_func_getloadavg=no",
                    "ac_cv_func_getpgid=no",
                    "ac_cv_func_getpgrp=no",
                    "ac_cv_func_getresgid=no",
                    "ac_cv_func_getresuid=no",
                    "ac_cv_func_getsid=no",
                    "ac_cv_func_lchmod=no",
                    "ac_cv_func_lchown=no",
                    "ac_cv_func_mkfifo=no",
                    "ac_cv_func_mkfifoat=no",
                    "ac_cv_func_mknod=no",
                    "ac_cv_func_mknodat=no",
                    "ac_cv_func_posix_fadvise=no",
                    "ac_cv_func_posix_fallocate=no",
                    "ac_cv_func_posix_spawn=no",
                    "ac_cv_func_pread=no",
                    "ac_cv_func_preadv=no",
                    "ac_cv_func_pwrite=no",
                    "ac_cv_func_pwritev=no",
                    "ac_cv_func_sigaltstack=no",
                    "ac_cv_func_sync=no",
                    "ac_cv_func_truncate=no",
                    "ac_cv_func_ttyname_r=no",
                    "ac_cv_func_utimensat=no",
                    "ac_cv_func_wait3=no",
                    "ac_cv_func_wait4=no",
                    "ac_cv_func_writev=no",
                )
            )
            + "\n"
        )
        env["CONFIG_SITE"] = str(config_site)
        env["PYTHONPATH"] = str(source / "Lib")
        run(
            [
                source / "configure",
                "--build=x86_64-pc-linux-gnu",
                "--host=x86_64-octagram",
                "--disable-ipv6",
                "--disable-test-modules",
                "--with-ensurepip=no",
                "--without-pymalloc",
                "--without-doc-strings",
                f"--with-build-python={host_python}",
            ],
            cwd=build,
            env=env,
        )
        patch_octagram_pyconfig(build)
        (build / "Modules" / "Setup.local").write_text(
            "*disabled*\n"
            "_posixsubprocess\n"
            "fcntl\n"
            "faulthandler\n"
            "spwd\n"
            "grp\n"
            "pwd\n"
            "readline\n"
        )
        run(["make", f"-j{os.cpu_count() or 2}"], cwd=build, env=env)

    print("CPython source preparation complete.")
    print(
        "Next blocker: cross-link CPython against user/runtime.c; "
        "the current user libc is deliberately incomplete."
    )
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except RuntimeError as error:
        print(f"ERROR: {error}", file=sys.stderr)
        sys.exit(1)
