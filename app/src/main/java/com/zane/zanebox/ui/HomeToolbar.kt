package com.zane.zanebox.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.layout.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.AccountTree
import androidx.compose.material.icons.outlined.Tune
import androidx.compose.material.icons.outlined.Dns
import androidx.compose.material.icons.outlined.PowerSettingsNew
import androidx.compose.material3.*
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.painterResource
import com.zane.zanebox.R
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.role
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.viewinterop.AndroidView

@Composable internal fun HomeToolbar(state:Int,onPage:(Int)->Unit,onToggle:()->Unit) {
    Box(Modifier.fillMaxWidth().navigationBarsPadding().padding(bottom=4.dp).height(127.dp).testTag("home_toolbar")) {
        AndroidView(factory={ZaneHomeToolbar(it).apply{importantForAccessibility=android.view.View.IMPORTANT_FOR_ACCESSIBILITY_NO}},modifier=Modifier.fillMaxSize())
        BoxWithConstraints(Modifier.fillMaxSize()) {
            val sideWidth=maxOf(96f,48f*androidx.compose.ui.platform.LocalDensity.current.fontScale+16f).dp.coerceAtMost((maxWidth-120.dp)/2)
            listOf(1 to .2f,2 to .8f).forEach { (page,fraction)->
                Column(Modifier.offset(x=24.dp+(maxWidth-48.dp)*fraction-sideWidth/2,y=52.dp).size(width=sideWidth,height=58.dp)
                    .clickable{onPage(page)}.testTag("tab_$page").padding(top=4.dp),
                    horizontalAlignment=Alignment.CenterHorizontally,verticalArrangement=Arrangement.Center) {
                    Icon(painterResource(if(page==2)R.drawable.zb_ref_ic_baseline_tune_24 else R.drawable.zb_ref_ic_anybox_smart_routing),null,Modifier.size(26.dp))
                    Spacer(Modifier.height(3.dp))
                    Text(uiText(if(page==2)"设置" else "智能分流"),fontSize=12.sp,lineHeight=12.sp,maxLines=1,
                        style=androidx.compose.ui.text.TextStyle(platformStyle=androidx.compose.ui.text.PlatformTextStyle(includeFontPadding=false)))
                }
            }
        }
        val label=uiText(listOf("连接","连接中…","断开","断开中…","连接").getOrElse(state){"同步中"})
        val description=uiText("代理开关")+"："+label
        Box(Modifier.align(Alignment.TopCenter).offset(y=10.dp).size(82.dp).testTag("connect_toggle")
            .semantics{contentDescription=description}.clip(CircleShape).clickable(enabled=state!=1 && state!=3,onClick=onToggle)) {
            AndroidView(factory={ZanePowerButton(it).apply{isClickable=false;isFocusable=false}},update={it.render(state,false)},modifier=Modifier.fillMaxSize())
        }
        Text(label,Modifier.align(Alignment.TopCenter).offset(y=92.dp).testTag("tab_0").clickable(enabled=state!=1 && state!=3,onClick=onToggle),fontSize=12.sp,lineHeight=16.sp)
    }
}

@OptIn(androidx.compose.foundation.ExperimentalFoundationApi::class)
@Composable internal fun HomeGroupChip(text:String,selected:Boolean,onClick:()->Unit,onLongClick:(()->Unit)?=null,active:Boolean=true,modifier:Modifier=Modifier) {
    val linear=skinPalette.name!="prism"
    val border=if(skinPalette.name=="linear")androidx.compose.foundation.BorderStroke(1.dp,if(selected)skinPrimary.copy(alpha=.7f) else skinDivider) else null
    Surface(color=if(selected && skinPalette.name=="linear")skinIconTile else if(selected && linear)skinSurfaceElevated else if(selected)MaterialTheme.colorScheme.primaryContainer else if(linear)skinSurface else MaterialTheme.colorScheme.surface,
        shape=if(linear)MaterialTheme.shapes.extraSmall else MaterialTheme.shapes.medium,
        border=border,
        modifier=modifier.semantics{this.selected=selected}.alpha(if(active)1f else .45f).combinedClickable(onClick=onClick,onLongClick=onLongClick)) {
        Box(Modifier.heightIn(min=if(linear)42.dp else 0.dp),contentAlignment=Alignment.Center){
            Text(text,Modifier.padding(horizontal=14.dp,vertical=9.dp),fontSize=12.sp,lineHeight=16.sp,maxLines=1,
                color=if(selected && skinPalette.name=="linear")skinOnSurface else if(linear)skinSecondaryText else MaterialTheme.colorScheme.onSurface,
                fontWeight=if(selected)FontWeight.SemiBold else FontWeight.Normal)
        }
    }
}

@Composable internal fun WhiteConnectionCard(
    selected:String,
    subtitle:String,
    state:Int,
    serviceMode:String,
    routeMode:String,
    onToggle:()->Unit,
    proxyCore:String="sing-box",
    onSelectCore:((String)->Unit)?=null,
) {
    val status=uiText(listOf("代理未连接","连接中…","代理已连接","断开中…","连接失败").getOrElse(state){"同步中"})
    val action=uiText(listOf("连接","连接中…","断开","断开中…","重试连接").getOrElse(state){"同步中"})
    val description=uiText("代理开关")+"："+action
    val dotColor=when(state) {
        2->skinSuccess
        1,3->skinPrimary
        4->skinError
        else->skinSecondaryText.copy(alpha=.55f)
    }
    UiCard(Modifier.testTag("white_connection_card")) {
        Column(Modifier.padding(16.dp),verticalArrangement=Arrangement.spacedBy(8.dp)) {
            Row(Modifier.fillMaxWidth(),verticalAlignment=Alignment.CenterVertically,horizontalArrangement=Arrangement.SpaceBetween) {
                Row(verticalAlignment=Alignment.CenterVertically,horizontalArrangement=Arrangement.spacedBy(8.dp)) {
                    Surface(color=dotColor,shape=CircleShape,modifier=Modifier.size(8.dp)) {}
                    Surface(color=if(skinPalette.name=="miro")androidx.compose.ui.graphics.Color(0xFFC3FAF5) else androidx.compose.ui.graphics.Color.Transparent,shape=MaterialTheme.shapes.extraSmall) {
                        Text(status,fontSize=12.sp,fontWeight=FontWeight.Medium,color=if(state==2)skinSuccess else if(state==4)skinError else skinSecondaryText,
                            modifier=Modifier.testTag("connection_status").padding(horizontal=if(skinPalette.name=="miro")8.dp else 0.dp,vertical=if(skinPalette.name=="miro")4.dp else 0.dp))
                    }
                }
                if(subtitle.isNotBlank())Surface(color=if(skinPalette.name=="miro")androidx.compose.ui.graphics.Color(0xFFFDE0F0) else skinSurfaceElevated,
                    border=if(skinPalette.name=="linear")androidx.compose.foundation.BorderStroke(1.dp,skinDivider) else null,
                    shape=MaterialTheme.shapes.extraSmall) {
                    Text(subtitle,modifier=Modifier.padding(horizontal=8.dp,vertical=3.dp),fontSize=11.sp,
                        fontFamily=androidx.compose.ui.text.font.FontFamily.Monospace,color=skinSecondaryText,maxLines=1,overflow=TextOverflow.Ellipsis)
                }
            }
            Text(selected,fontFamily=MaterialTheme.typography.titleLarge.fontFamily,fontSize=18.sp,fontWeight=FontWeight.SemiBold,maxLines=2,overflow=TextOverflow.Ellipsis,modifier=Modifier.testTag("home_selected_node"))
            if(onSelectCore!=null) {
                Surface(color=skinBackground,shape=MaterialTheme.shapes.small,border=androidx.compose.foundation.BorderStroke(1.dp,skinDivider)) {
                    Row(Modifier.fillMaxWidth().padding(3.dp).testTag("core_switcher"),horizontalArrangement=Arrangement.spacedBy(4.dp)) {
                        listOf("sing-box" to "sing-box","xray" to "Xray","mihomo" to "mihomo").forEach { (id,coreLabel)->
                            val isActive=proxyCore.equals(id,ignoreCase=true)
                            Surface(color=if(isActive)skinPrimary else androidx.compose.ui.graphics.Color.Transparent,
                                shape=MaterialTheme.shapes.extraSmall,
                                modifier=Modifier.weight(1f).clip(MaterialTheme.shapes.extraSmall).clickable(enabled=state!=1 && state!=3){onSelectCore(id)}.testTag("core_chip_$id")) {
                                Box(Modifier.heightIn(min=34.dp),contentAlignment=Alignment.Center) {
                                    Text(coreLabel,fontSize=11.sp,fontFamily=androidx.compose.ui.text.font.FontFamily.Monospace,
                                        fontWeight=if(isActive)FontWeight.SemiBold else FontWeight.Medium,
                                        color=if(isActive)skinPalette.onPrimary else skinSecondaryText,maxLines=1)
                                }
                            }
                        }
                    }
                }
            }
            Row(Modifier.fillMaxWidth().padding(top=4.dp),verticalAlignment=Alignment.CenterVertically) {
                Text(uiText(if(serviceMode=="vpn")"VPN 模式" else "本地代理")+" · "+uiText(when(routeMode){"global"->"全局";"direct"->"直连";else->"智能分流"}),fontSize=12.sp,color=skinSecondaryText,modifier=Modifier.weight(1f))
                Spacer(Modifier.width(12.dp))
                Button(onClick=onToggle,enabled=state!=1 && state!=3,shape=MaterialTheme.shapes.small,
                    colors=ButtonDefaults.buttonColors(containerColor=if(state==2)skinSuccess else skinPrimary,contentColor=skinPalette.onPrimary),
                    modifier=Modifier.heightIn(min=44.dp).testTag("connect_toggle").semantics{contentDescription=description}) {
                    Icon(Icons.Outlined.PowerSettingsNew,null,Modifier.size(18.dp));Spacer(Modifier.width(6.dp));Text(action,fontWeight=FontWeight.SemiBold)
                }
            }
        }
    }
}

@Composable internal fun WhiteNavigation(page:Int,onPage:(Int)->Unit) {
    Surface(color=skinSurface) {
        Column { HorizontalDivider(thickness=1.dp,color=skinDivider)
            Row(Modifier.fillMaxWidth().navigationBarsPadding().padding(horizontal=8.dp,vertical=6.dp)) {
                listOf("节点" to Icons.Outlined.Dns,"智能分流" to Icons.Outlined.AccountTree,"设置" to Icons.Outlined.Tune).forEachIndexed { index,(label,icon)->
                    val activeTint=if(skinPalette.name=="linear")skinPrimaryDark else skinPrimary
                    Surface(color=if(page==index && skinPalette.name=="miro")skinIconTile else if(page==index)skinSurfaceElevated else androidx.compose.ui.graphics.Color.Transparent,shape=MaterialTheme.shapes.small,
                        modifier=Modifier.weight(1f).semantics{selected=page==index;role=Role.Tab}.clickable{onPage(index)}.testTag(if(index==0)"home_tab" else "tab_$index")) {
                        Column(Modifier.heightIn(min=60.dp).padding(vertical=7.dp),horizontalAlignment=Alignment.CenterHorizontally,verticalArrangement=Arrangement.spacedBy(4.dp)) {
                            Icon(icon,null,Modifier.size(22.dp),tint=if(page==index)activeTint else skinSecondaryText)
                            Text(uiText(label),fontSize=12.sp,fontWeight=if(page==index)FontWeight.SemiBold else FontWeight.Normal,color=if(page==index)activeTint else skinSecondaryText,maxLines=1)
                        }
                    }
                }
            }
        }
    }
}
