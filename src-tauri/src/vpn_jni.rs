use jni::{
    objects::{JObject, JString},
    sys::jint,
    JNIEnv,
};

// JNI 错误转为 Java 异常，由 VPN 服务处理并关闭 TUN。
#[no_mangle]
pub extern "system" fn Java_i_can_fly_vpn_XrayVpnBackend_nativeStart(
    mut env: JNIEnv,
    _this: JObject,
    sets_json: JString,
    fd: jint,
    mtu: jint,
    inbound: jint,
) {
    let result = (|| -> Result<(), String> {
        let json: String = env
            .get_string(&sets_json)
            .map_err(|e| e.to_string())?
            .into();
        let sets = serde_json::from_str::<Vec<String>>(&json).map_err(|e| e.to_string())?;
        let index = usize::try_from(inbound).map_err(|e| e.to_string())?;
        crate::xray::start_vpn(sets, fd, mtu, index)
    })();
    if let Err(error) = result {
        let _ = env.throw_new("java/lang/IllegalStateException", error);
    }
}

#[no_mangle]
pub extern "system" fn Java_i_can_fly_vpn_XrayVpnBackend_nativeStop(
    mut env: JNIEnv,
    _this: JObject,
) {
    if let Err(error) = crate::xray::stop_vpn() {
        let _ = env.throw_new("java/lang/IllegalStateException", error);
    }
}
