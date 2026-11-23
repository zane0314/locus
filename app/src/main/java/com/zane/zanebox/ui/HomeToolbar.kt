package com.zane.zanebox.ui

import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.layout.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.AccountTree
import androidx.compose.material.icons.outlined.ArrowDropDown
import androidx.compose.material.icons.outlined.Tune
import androidx.compose.material.icons.outlined.Dns
import androidx.compose.material.icons.outlined.PowerSettingsNew
import androidx.compose.material3.*
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
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
import androidx.compose.ui.text.font.FontFamily
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
    val border=when {
        !linear -> null
        selected && skinPalette.name=="miro" -> BorderStroke(1.dp,skinPrimary)
        selected -> BorderStroke(1.dp,skinPrimary.copy(alpha=.75f))
        else -> BorderStroke(1.dp,skinDivider)
    }
    val containerColor=when {
        selected && linear -> skinPalette.chipSelectedBg
        selected -> MaterialTheme.colorScheme.primaryContainer
        linear -> skinSurface
        else -> MaterialTheme.colorScheme.surface
    }
    val textColor=when {
        selected && linear -> skinOnSurface
        linear -> skinSecondaryText
        else -> MaterialTheme.colorScheme.onSurface
    }
    Surface(color=containerColor,
        shape=if(linear)MaterialTheme.shapes.extraSmall else MaterialTheme.shapes.medium,
        border=border,
        modifier=modifier.semantics{this.selected=selected}.alpha(if(active)1f else .45f).combinedClickable(onClick=onClick,onLongClick=onLongClick)) {
        Box(Modifier.heightIn(min=if(linear)36.dp else 0.dp),contentAlignment=Alignment.Center){
            Text(text,Modifier.padding(horizontal=12.dp,vertical=7.dp),fontSize=12.sp,lineHeight=16.sp,maxLines=1,
                color=textColor,
                fontWeight=if(selected)FontWeight.Medium else FontWeight.Normal)
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
    val targetDotColor=when(state) {
        2->skinSuccess
        1,3->skinPrimary
        4->skinError
        else->skinPalette.textTertiary
    }
    val dotColor by animateColorAsState(targetDotColor,tween(180),label="statusDot")
    val targetBorderColor=when {
        state==2 -> skinSuccess.copy(alpha=.55f)
        state==4 -> skinError.copy(alpha=.55f)
        skinPalette.name=="linear" -> skinPalette.hairlineStrong.copy(alpha=.65f)
        else -> skinDivider
    }
    val cardBorderColor by animateColorAsState(targetBorderColor,tween(180),label="cardBorder")
    val cores=listOf("sing-box" to "sing-box","xray" to "Xray","mihomo" to "mihomo")
    val currentCoreLabel=cores.firstOrNull { it.first.equals(proxyCore,ignoreCase=true) }?.second ?: proxyCore
    var coreMenuOpen by remember { mutableStateOf(false) }

    Surface(
        modifier=Modifier.fillMaxWidth().testTag("white_connection_card"),
        shape=MaterialTheme.shapes.large,
        color=MaterialTheme.colorScheme.surface,
        border=BorderStroke(1.dp,cardBorderColor),
    ) {
        Column(Modifier.padding(horizontal=14.dp,vertical=12.dp),verticalArrangement=Arrangement.spacedBy(8.dp)) {
            Row(Modifier.fillMaxWidth(),verticalAlignment=Alignment.CenterVertically,horizontalArrangement=Arrangement.SpaceBetween) {
                Row(verticalAlignment=Alignment.CenterVertically,horizontalArrangement=Arrangement.spacedBy(7.dp)) {
                    Surface(color=dotColor,shape=CircleShape,modifier=Modifier.size(8.dp)) {}
                    Text(status,fontSize=12.sp,fontWeight=FontWeight.Medium,
                        color=if(state==2)skinSuccess else if(state==4)skinError else skinSecondaryText,
                        modifier=Modifier.testTag("connection_status"))
                }
                if(subtitle.isNotBlank())Surface(color=skinSurfaceElevated,
                    border=BorderStroke(.5.dp,skinDivider),
                    shape=MaterialTheme.shapes.extraSmall) {
                    Text(subtitle,modifier=Modifier.padding(horizontal=7.dp,vertical=2.dp),fontSize=11.sp,
                        fontFamily=FontFamily.Monospace,color=skinSecondaryText,maxLines=1,overflow=TextOverflow.Ellipsis)
                }
            }
            Text(selected,fontFamily=MaterialTheme.typography.titleLarge.fontFamily,fontSize=17.sp,lineHeight=22.sp,
                fontWeight=FontWeight.SemiBold,maxLines=1,overflow=TextOverflow.Ellipsis,modifier=Modifier.testTag("home_selected_node"))
            Row(Modifier.fillMaxWidth(),verticalAlignment=Alignment.CenterVertically,horizontalArrangement=Arrangement.spacedBy(8.dp)) {
                Row(Modifier.weight(1f),verticalAlignment=Alignment.CenterVertically,horizontalArrangement=Arrangement.spacedBy(8.dp)) {
                    Text(uiText(if(serviceMode=="vpn")"VPN 模式" else "本地代理")+" · "+uiText(when(routeMode){"global"->"全局";"direct"->"直连";else->"智能分流"}),
                        fontSize=12.sp,color=skinSecondaryText,maxLines=1,overflow=TextOverflow.Ellipsis,modifier=Modifier.weight(1f,fill=false))
                    if(onSelectCore!=null) {
                        Box {
                            Surface(
                                color=skinSurfaceElevated,
                                shape=MaterialTheme.shapes.extraSmall,
                                border=BorderStroke(1.dp,skinDivider),
                                modifier=Modifier.clip(MaterialTheme.shapes.extraSmall)
                                    .clickable(enabled=state!=1 && state!=3){coreMenuOpen=true}
                                    .testTag("core_switcher"),
                            ) {
                                Row(Modifier.heightIn(min=28.dp).padding(start=8.dp,end=4.dp),verticalAlignment=Alignment.CenterVertically) {
                                    Text(currentCoreLabel,fontSize=11.sp,fontFamily=FontFamily.Monospace,fontWeight=FontWeight.Medium,color=skinOnSurface,maxLines=1)
                                    Icon(Icons.Outlined.ArrowDropDown,null,Modifier.size(16.dp),tint=skinSecondaryText)
                                }
                            }
                            DropdownMenu(expanded=coreMenuOpen,onDismissRequest={coreMenuOpen=false}) {
                                cores.forEach { (id,coreLabel) ->
                                    val isActive=proxyCore.equals(id,ignoreCase=true)
                                    DropdownMenuItem(
                                        text={Text(coreLabel,fontFamily=FontFamily.Monospace,fontWeight=if(isActive)FontWeight.SemiBold else FontWeight.Normal)},
                                        onClick={coreMenuOpen=false;onSelectCore(id)},
                                        modifier=Modifier.testTag("core_chip_$id"),
                                    )
                                }
                            }
                        }
                    }
                }
                val isConnected=state==2
                val btnContainer by animateColorAsState(if(isConnected)skinSurfaceElevated else skinPrimary,tween(180),label="connectBtnBg")
                val btnContent by animateColorAsState(if(isConnected)skinOnSurface else skinPalette.onPrimary,tween(180),label="connectBtnFg")
                Button(
                    onClick=onToggle,
                    enabled=state!=1 && state!=3,
                    shape=MaterialTheme.shapes.small,
                    border=if(isConnected)BorderStroke(1.dp,skinPalette.hairlineStrong) else null,
                    contentPadding=PaddingValues(horizontal=14.dp,vertical=8.dp),
                    colors=ButtonDefaults.buttonColors(containerColor=btnContainer,contentColor=btnContent),
                    modifier=Modifier.heightIn(min=40.dp).testTag("connect_toggle").semantics{contentDescription=description},
                ) {
                    Icon(Icons.Outlined.PowerSettingsNew,null,Modifier.size(16.dp))
                    Spacer(Modifier.width(6.dp))
                    Text(action,fontSize=13.sp,fontWeight=FontWeight.SemiBold,maxLines=1)
                }
            }
        }
    }
}

@Composable internal fun WhiteNavigation(page:Int,onPage:(Int)->Unit) {
    Surface(color=skinSurface) {
        Column { HorizontalDivider(thickness=1.dp,color=skinDivider)
            Row(Modifier.fillMaxWidth().navigationBarsPadding().padding(horizontal=6.dp,vertical=2.dp)) {
                listOf("节点" to Icons.Outlined.Dns,"智能分流" to Icons.Outlined.AccountTree,"设置" to Icons.Outlined.Tune).forEachIndexed { index,(label,icon)->
                    val activeTint=if(skinPalette.name=="linear")skinPrimaryDark else skinPrimary
                    val pillColor=if(page==index && skinPalette.name=="miro")skinIconTile else if(page==index)skinSurfaceElevated else androidx.compose.ui.graphics.Color.Transparent
                    Surface(color=androidx.compose.ui.graphics.Color.Transparent,shape=MaterialTheme.shapes.small,
                        modifier=Modifier.weight(1f).semantics{selected=page==index;role=Role.Tab}.clickable{onPage(index)}.testTag(if(index==0)"home_tab" else "tab_$index")) {
                        Column(Modifier.heightIn(min=51.dp).padding(vertical=3.dp,horizontal=2.dp),horizontalAlignment=Alignment.CenterHorizontally,verticalArrangement=Arrangement.spacedBy(2.dp)) {
                            Surface(color=pillColor,shape=MaterialTheme.shapes.extraSmall) {
                                Icon(icon,null,Modifier.padding(horizontal=12.dp,vertical=3.dp).size(20.dp),tint=if(page==index)activeTint else skinSecondaryText)
                            }
                            Text(uiText(label),fontSize=12.sp,lineHeight=15.sp,fontWeight=if(page==index)FontWeight.Medium else FontWeight.Normal,color=if(page==index)activeTint else skinSecondaryText,maxLines=1)
                        }
                    }
                }
            }
        }
    }
}
