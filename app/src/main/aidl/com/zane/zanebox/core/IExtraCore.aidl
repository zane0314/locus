package com.zane.zanebox.core;

import com.zane.zanebox.core.IExtraCoreProtect;

interface IExtraCore {
    String validate(String kind, String config);
    String start(String kind, String config, IExtraCoreProtect protector);
    String stop();
    String version(String kind);
}
