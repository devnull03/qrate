#include <windows.h>
#include <shlobj.h>
#include <shlwapi.h>
#include <wrl.h>
#include <new>
#include <string>

using Microsoft::WRL::ComPtr;
using Microsoft::WRL::Make;
using Microsoft::WRL::RuntimeClass;
using Microsoft::WRL::RuntimeClassFlags;
using Microsoft::WRL::ClassicCom;

extern "C" IMAGE_DOS_HEADER __ImageBase;
constexpr CLSID FolderCommandId = {
    0x78b41856, 0x4b52, 0x45c6, {0x9f, 0x05, 0x04, 0xdf, 0x92, 0x46, 0x11, 0xea}};
static LONG objects = 0;

static HRESULT executable_path(wchar_t (&path)[32768]) {
    const DWORD length = GetModuleFileNameW(reinterpret_cast<HMODULE>(&__ImageBase), path, 32768);
    if (!length) return HRESULT_FROM_WIN32(GetLastError());
    if (length == 32768) return HRESULT_FROM_WIN32(ERROR_INSUFFICIENT_BUFFER);
    wchar_t* filename = wcsrchr(path, L'\\');
    if (!filename) return E_FAIL;
    return wcscpy_s(filename + 1, 32768 - static_cast<size_t>(filename + 1 - path), L"qrate.exe") == 0
        ? S_OK : HRESULT_FROM_WIN32(ERROR_INSUFFICIENT_BUFFER);
}

class FolderCommand final : public RuntimeClass<RuntimeClassFlags<ClassicCom>, IExplorerCommand, IObjectWithSite> {
    ComPtr<IUnknown> site_;

    HRESULT target(IShellItemArray* selection, IShellItem** item) {
        *item = nullptr;
        DWORD count = 0;
        if (selection) {
            const HRESULT result = selection->GetCount(&count);
            if (FAILED(result)) return result;
        }
        if (count == 1) return selection->GetItemAt(0, item);
        if (count > 1 || !site_) return E_INVALIDARG;

        // Folder-background commands have no selection; resolve the view's current folder.
        ComPtr<IServiceProvider> services;
        HRESULT result = site_.As(&services);
        if (FAILED(result)) return result;
        ComPtr<IShellBrowser> browser;
        result = services->QueryService(SID_STopLevelBrowser, IID_PPV_ARGS(&browser));
        if (FAILED(result)) return result;
        ComPtr<IShellView> view;
        result = browser->QueryActiveShellView(&view);
        if (FAILED(result)) return result;
        ComPtr<IFolderView> folder_view;
        result = view.As(&folder_view);
        if (FAILED(result)) return result;
        ComPtr<IShellFolder> folder;
        result = folder_view->GetFolder(IID_PPV_ARGS(&folder));
        if (FAILED(result)) return result;
        PIDLIST_ABSOLUTE location = nullptr;
        result = SHGetIDListFromObject(folder.Get(), &location);
        if (SUCCEEDED(result)) result = SHCreateItemFromIDList(location, IID_PPV_ARGS(item));
        CoTaskMemFree(location);
        return result;
    }

public:
    FolderCommand() { InterlockedIncrement(&objects); }
    ~FolderCommand() { InterlockedDecrement(&objects); }

    IFACEMETHODIMP SetSite(IUnknown* site) override {
        site_ = site;
        return S_OK;
    }
    IFACEMETHODIMP GetSite(REFIID iid, void** object) override {
        if (!object) return E_POINTER;
        *object = nullptr;
        return site_ ? site_->QueryInterface(iid, object) : E_FAIL;
    }
    IFACEMETHODIMP GetTitle(IShellItemArray*, PWSTR* title) override {
        return SHStrDupW(L"Open with qrate", title);
    }
    IFACEMETHODIMP GetIcon(IShellItemArray*, PWSTR* icon) override {
        if (!icon) return E_POINTER;
        *icon = nullptr;
        wchar_t path[32768];
        HRESULT result = executable_path(path);
        if (FAILED(result)) return result;
        if (wcscat_s(path, L",0") != 0) return HRESULT_FROM_WIN32(ERROR_INSUFFICIENT_BUFFER);
        return SHStrDupW(path, icon);
    }
    IFACEMETHODIMP GetToolTip(IShellItemArray*, PWSTR* tooltip) override {
        if (!tooltip) return E_POINTER;
        *tooltip = nullptr;
        return E_NOTIMPL;
    }
    IFACEMETHODIMP GetCanonicalName(GUID* name) override {
        if (!name) return E_POINTER;
        *name = FolderCommandId;
        return S_OK;
    }
    IFACEMETHODIMP GetState(IShellItemArray* selection, BOOL, EXPCMDSTATE* state) override {
        if (!state) return E_POINTER;
        *state = ECS_HIDDEN;
        ComPtr<IShellItem> folder;
        SFGAOF attributes = 0;
        constexpr SFGAOF required = SFGAO_FILESYSTEM | SFGAO_FOLDER;
        if (SUCCEEDED(target(selection, &folder)) &&
            SUCCEEDED(folder->GetAttributes(required, &attributes)) &&
            (attributes & required) == required) {
            *state = ECS_ENABLED;
        }
        return S_OK;
    }
    IFACEMETHODIMP Invoke(IShellItemArray* selection, IBindCtx*) override {
        ComPtr<IShellItem> folder;
        HRESULT result = target(selection, &folder);
        if (FAILED(result)) return result;
        PWSTR raw_path = nullptr;
        result = folder->GetDisplayName(SIGDN_FILESYSPATH, &raw_path);
        if (FAILED(result)) return result;
        try {
            std::wstring path(raw_path);
            CoTaskMemFree(raw_path);
            raw_path = nullptr;
            // A drive root ends in a backslash, which must be doubled before the closing quote.
            const size_t last = path.find_last_not_of(L'\\');
            const size_t trailing = last == std::wstring::npos ? path.size() : path.size() - last - 1;
            std::wstring arguments = L"\"" + path + std::wstring(trailing, L'\\') + L"\"";
            wchar_t executable[32768];
            result = executable_path(executable);
            if (FAILED(result)) return result;
            SHELLEXECUTEINFOW launch = {};
            launch.cbSize = sizeof(launch);
            launch.fMask = SEE_MASK_FLAG_NO_UI | SEE_MASK_NOASYNC;
            launch.lpVerb = L"open";
            launch.lpFile = executable;
            launch.lpParameters = arguments.c_str();
            launch.nShow = SW_SHOWNORMAL;
            return ShellExecuteExW(&launch) ? S_OK : HRESULT_FROM_WIN32(GetLastError());
        } catch (const std::bad_alloc&) {
            CoTaskMemFree(raw_path);
            return E_OUTOFMEMORY;
        }
    }
    IFACEMETHODIMP GetFlags(EXPCMDFLAGS* flags) override {
        if (!flags) return E_POINTER;
        *flags = ECF_DEFAULT;
        return S_OK;
    }
    IFACEMETHODIMP EnumSubCommands(IEnumExplorerCommand** commands) override {
        if (!commands) return E_POINTER;
        *commands = nullptr;
        return E_NOTIMPL;
    }
};

class CommandFactory final : public RuntimeClass<RuntimeClassFlags<ClassicCom>, IClassFactory> {
public:
    CommandFactory() { InterlockedIncrement(&objects); }
    ~CommandFactory() { InterlockedDecrement(&objects); }
    IFACEMETHODIMP CreateInstance(IUnknown* outer, REFIID iid, void** object) override {
        if (!object) return E_POINTER;
        *object = nullptr;
        if (outer) return CLASS_E_NOAGGREGATION;
        const auto command = Make<FolderCommand>();
        return command ? command->QueryInterface(iid, object) : E_OUTOFMEMORY;
    }
    IFACEMETHODIMP LockServer(BOOL lock) override {
        if (lock) InterlockedIncrement(&objects);
        else InterlockedDecrement(&objects);
        return S_OK;
    }
};

extern "C" HRESULT __stdcall DllGetClassObject(REFCLSID clsid, REFIID iid, void** object) {
    if (!object) return E_POINTER;
    *object = nullptr;
    if (clsid != FolderCommandId) return CLASS_E_CLASSNOTAVAILABLE;
    const auto factory = Make<CommandFactory>();
    return factory ? factory->QueryInterface(iid, object) : E_OUTOFMEMORY;
}

extern "C" HRESULT __stdcall DllCanUnloadNow() {
    return InterlockedCompareExchange(&objects, 0, 0) == 0 ? S_OK : S_FALSE;
}
