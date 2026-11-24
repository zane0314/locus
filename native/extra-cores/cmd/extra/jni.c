#include <jni.h>
#include <pthread.h>
#include <stdlib.h>
#include <string.h>

extern char* ExtraVersion(char* kind);
extern char* ExtraValidate(char* kind, char* config);
extern char* ExtraStart(char* kind, char* config);
extern char* ExtraStop(void);
extern void ExtraFree(char* p);

static JavaVM* g_vm = NULL;
static pthread_mutex_t g_protect_mu = PTHREAD_MUTEX_INITIALIZER;
static jobject g_protector = NULL;
static jmethodID g_protect_method = NULL;

JNIEXPORT jint JNICALL JNI_OnLoad(JavaVM* vm, void* reserved) {
	(void)reserved;
	g_vm = vm;
	return JNI_VERSION_1_6;
}

static void clear_protector(JNIEnv* env) {
	pthread_mutex_lock(&g_protect_mu);
	if (g_protector != NULL) {
		(*env)->DeleteGlobalRef(env, g_protector);
		g_protector = NULL;
	}
	g_protect_method = NULL;
	pthread_mutex_unlock(&g_protect_mu);
}

static char* copy_jbyte_array(JNIEnv* env, jbyteArray arr) {
	if (arr == NULL) {
		return NULL;
	}
	jsize len = (*env)->GetArrayLength(env, arr);
	char* buf = (char*)malloc((size_t)len + 1);
	if (buf == NULL) {
		return NULL;
	}
	if (len > 0) {
		(*env)->GetByteArrayRegion(env, arr, 0, len, (jbyte*)buf);
		if ((*env)->ExceptionCheck(env)) {
			free(buf);
			return NULL;
		}
	}
	buf[len] = '\0';
	return buf;
}

static jbyteArray to_jbyte_array_const(JNIEnv* env, const char* str) {
	size_t len = str != NULL ? strlen(str) : 0;
	jbyteArray out = (*env)->NewByteArray(env, (jsize)len);
	if (out != NULL && len > 0) {
		(*env)->SetByteArrayRegion(env, out, 0, (jsize)len, (const jbyte*)str);
	}
	return out;
}

static jbyteArray to_jbyte_array(JNIEnv* env, char* str) {
	jbyteArray out = to_jbyte_array_const(env, str);
	if (str != NULL) {
		ExtraFree(str);
	}
	return out;
}

int extra_protect(int fd) {
	JavaVM* vm = g_vm;
	if (vm == NULL) {
		return 0;
	}
	JNIEnv* env = NULL;
	jint status = (*vm)->GetEnv(vm, (void**)&env, JNI_VERSION_1_6);
	int attached = 0;
	if (status == JNI_EDETACHED) {
		if ((*vm)->AttachCurrentThread(vm, &env, NULL) != JNI_OK) {
			return 0;
		}
		attached = 1;
	} else if (status != JNI_OK) {
		return 0;
	}

	pthread_mutex_lock(&g_protect_mu);
	jobject local_ref = g_protector != NULL ? (*env)->NewLocalRef(env, g_protector) : NULL;
	jmethodID method = g_protect_method;
	pthread_mutex_unlock(&g_protect_mu);

	int ok = 0;
	if (local_ref != NULL && method != NULL) {
		jboolean res = (*env)->CallBooleanMethod(env, local_ref, method, (jint)fd);
		ok = (res == JNI_TRUE) ? 1 : 0;
		if ((*env)->ExceptionCheck(env)) {
			(*env)->ExceptionClear(env);
			ok = 0;
		}
		(*env)->DeleteLocalRef(env, local_ref);
	} else {
		if ((*env)->ExceptionCheck(env)) {
			(*env)->ExceptionClear(env);
		}
	}

	if (attached) {
		(*vm)->DetachCurrentThread(vm);
	}
	return ok;
}

JNIEXPORT jbyteArray JNICALL
Java_com_zane_zanebox_core_ExtraCoreNative_nativeVersion(JNIEnv* env, jclass cls, jbyteArray kind) {
	(void)cls;
	char* k = copy_jbyte_array(env, kind);
	if (k == NULL) {
		return to_jbyte_array_const(env, "");
	}
	char* res = ExtraVersion(k);
	free(k);
	return to_jbyte_array(env, res);
}

JNIEXPORT jbyteArray JNICALL
Java_com_zane_zanebox_core_ExtraCoreNative_nativeValidate(JNIEnv* env, jclass cls, jbyteArray kind, jbyteArray config) {
	(void)cls;
	char* k = copy_jbyte_array(env, kind);
	char* c = copy_jbyte_array(env, config);
	if (k == NULL || c == NULL) {
		free(k);
		free(c);
		return to_jbyte_array_const(env, "invalid native validate arguments");
	}
	char* res = ExtraValidate(k, c);
	free(k);
	free(c);
	return to_jbyte_array(env, res);
}

JNIEXPORT jbyteArray JNICALL
Java_com_zane_zanebox_core_ExtraCoreNative_nativeStart(JNIEnv* env, jclass cls, jbyteArray kind, jbyteArray config, jobject protector) {
	(void)cls;
	clear_protector(env);
	if (protector != NULL) {
		jclass pcls = (*env)->GetObjectClass(env, protector);
		if (pcls == NULL) {
			return to_jbyte_array_const(env, "missing socket protector class");
		}
		jmethodID mid = (*env)->GetMethodID(env, pcls, "protect", "(I)Z");
		(*env)->DeleteLocalRef(env, pcls);
		if (mid == NULL) {
			return to_jbyte_array_const(env, "missing protect(I)Z method");
		}
		jobject gref = (*env)->NewGlobalRef(env, protector);
		if (gref == NULL) {
			return to_jbyte_array_const(env, "failed to retain socket protector");
		}
		pthread_mutex_lock(&g_protect_mu);
		g_protector = gref;
		g_protect_method = mid;
		pthread_mutex_unlock(&g_protect_mu);
	}

	char* k = copy_jbyte_array(env, kind);
	char* c = copy_jbyte_array(env, config);
	if (k == NULL || c == NULL) {
		free(k);
		free(c);
		clear_protector(env);
		return to_jbyte_array_const(env, "invalid native start arguments");
	}
	char* res = ExtraStart(k, c);
	free(k);
	free(c);
	if (res == NULL || res[0] != '\0') {
		clear_protector(env);
	}
	return to_jbyte_array(env, res);
}

JNIEXPORT jbyteArray JNICALL
Java_com_zane_zanebox_core_ExtraCoreNative_nativeStop(JNIEnv* env, jclass cls) {
	(void)cls;
	char* res = ExtraStop();
	clear_protector(env);
	return to_jbyte_array(env, res);
}
