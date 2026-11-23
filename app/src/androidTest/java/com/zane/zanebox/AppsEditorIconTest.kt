package com.zane.zanebox

import androidx.compose.material3.MaterialTheme
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createComposeRule
import com.zane.zanebox.ui.AppsEditor
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test

class AppsEditorIconTest {
    @get:Rule val compose=createComposeRule()

    @Test fun ownApplicationIconLoadsAndSelectionStillSaves() {
        val pkg=androidx.test.platform.app.InstrumentationRegistry.getInstrumentation().targetContext.packageName
        var saved:Set<String>?=null
        compose.setContent {
            MaterialTheme { AppsEditor(emptySet(),onDismiss={},save={saved=it}) }
        }
        compose.onNodeWithTag("apps_search").performTextInput(pkg)
        compose.waitUntil(10000) { compose.onAllNodesWithTag("app_$pkg").fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithTag("subpage_list").performScrollToNode(hasTestTag("app_$pkg"))
        try {
            compose.waitUntil(10000) { compose.onAllNodesWithTag("app_icon_loaded_$pkg",useUnmergedTree=true).fetchSemanticsNodes().isNotEmpty() }
        } catch (failure:Throwable) {
            compose.onRoot(useUnmergedTree=true).printToLog("AppsEditorIconTest")
            throw failure
        }
        compose.onNodeWithTag("app_icon_loaded_$pkg",useUnmergedTree=true).assertIsDisplayed()
        compose.onNodeWithTag("app_$pkg").performClick()
        compose.onNodeWithTag("subpage_list").performScrollToIndex(0)
        compose.waitUntil(5000) { compose.onNodeWithTag("apps_save").isDisplayed() }
        compose.onNodeWithTag("apps_save").performClick()
        compose.runOnIdle { assertEquals(setOf(pkg),saved) }
    }
}
