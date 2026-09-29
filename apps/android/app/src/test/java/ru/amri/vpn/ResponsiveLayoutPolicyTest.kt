package ru.amri.vpn

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ResponsiveLayoutPolicyTest {
    @Test
    fun narrowPhoneUsesCompactTouchSafeControls() {
        val layout = ResponsiveLayoutPolicy.resolve(widthDp = 320, heightDp = 640, fontScale = 1f)
        assertEquals(12, layout.horizontalPaddingDp)
        assertEquals(48, layout.iconButtonDp)
        assertEquals(108, layout.powerButtonDp)
        assertFalse(layout.showBrandIcon)
        assertFalse(layout.showSubtitle)
    }

    @Test
    fun largeTextDropsOptionalHeaderContentBeforeClippingActions() {
        val layout = ResponsiveLayoutPolicy.resolve(widthDp = 360, heightDp = 740, fontScale = 1.5f)
        assertFalse(layout.showBrandIcon)
        assertFalse(layout.showSubtitle)
        assertTrue(layout.iconButtonDp >= 48)
    }

    @Test
    fun landscapeCompactsVerticalHero() {
        val layout = ResponsiveLayoutPolicy.resolve(widthDp = 800, heightDp = 420, fontScale = 1f)
        assertEquals(96, layout.powerButtonDp)
        assertTrue(layout.showBrandIcon)
    }

    @Test
    fun tabletKeepsReadableCenteredContentWidth() {
        val layout = ResponsiveLayoutPolicy.resolve(widthDp = 1280, heightDp = 900, fontScale = 1f)
        assertEquals(720, layout.maxContentWidthDp)
        assertEquals(28, layout.horizontalPaddingDp)
        assertTrue(layout.showSubtitle)
    }
}
