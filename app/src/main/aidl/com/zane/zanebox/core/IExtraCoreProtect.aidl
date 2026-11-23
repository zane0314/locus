package com.zane.zanebox.core;

import android.os.ParcelFileDescriptor;

interface IExtraCoreProtect {
    boolean protect(in ParcelFileDescriptor fd);
}
