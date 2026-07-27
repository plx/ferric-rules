;;; ============================================================
;;; Mobile App Engagement Rules
;;;
;;; Decides which (if any) prompt to show a user during an app
;;; session: rating requests, upsells, paywalls, retention
;;; offers, signup incentives, or social-share credits.
;;;
;;; Only one prompt fires per evaluation. Salience controls
;;; priority; the (prompt-shown) guard prevents lower-priority
;;; rules from activating once a decision is made.
;;; ============================================================

;;; ------------------------------------------------------------
;;; Suppress everything after a bad experience
;;; ------------------------------------------------------------
(defrule suppress-after-crash
    (declare (salience 100))
    (has-crashed yes)
    =>
    (assert (prompt-suppressed))
    (assert (prompt-shown)))

;;; ------------------------------------------------------------
;;; Paywall: free user hit a premium-only feature
;;; ------------------------------------------------------------
(defrule show-paywall
    (declare (salience 90))
    (user-tier free)
    (accessed-premium-feature)
    (not (prompt-shown))
    (not (prompt-suppressed))
    =>
    (assert (show paywall))
    (assert (prompt-shown))
    (printout t "ACTION: paywall" crlf))

;;; ------------------------------------------------------------
;;; Signup incentive for brand-new users
;;; ------------------------------------------------------------
(defrule offer-signup-incentive
    (declare (salience 70))
    (user-tier free)
    (session-count ?s)
    (test (<= ?s 3))
    (not (prompt-shown))
    (not (prompt-suppressed))
    =>
    (assert (show signup-incentive))
    (assert (prompt-shown))
    (printout t "ACTION: signup-incentive" crlf))

;;; ------------------------------------------------------------
;;; Retention discount for lapsed users
;;; ------------------------------------------------------------
(defrule offer-retention-discount
    (declare (salience 60))
    (days-since-last-open ?d)
    (test (>= ?d 7))
    (not (prompt-shown))
    (not (prompt-suppressed))
    =>
    (assert (show retention-discount))
    (assert (prompt-shown))
    (printout t "ACTION: retention-discount" crlf))

;;; ------------------------------------------------------------
;;; "Enjoying this? Rate us!" — engaged, happy users only
;;; ------------------------------------------------------------
(defrule prompt-app-rating
    (declare (salience 50))
    (session-count ?s)
    (test (>= ?s 10))
    (days-since-install ?d)
    (test (>= ?d 7))
    (has-rated no)
    (not (has-crashed yes))
    (not (prompt-shown))
    (not (prompt-suppressed))
    =>
    (assert (show rate-app))
    (assert (prompt-shown))
    (printout t "ACTION: rate-app" crlf))

;;; ------------------------------------------------------------
;;; Upsell free -> paid
;;; ------------------------------------------------------------
(defrule upsell-to-paid
    (declare (salience 40))
    (user-tier free)
    (session-count ?s)
    (test (>= ?s 5))
    (feature-usage high)
    (not (prompt-shown))
    (not (prompt-suppressed))
    =>
    (assert (show upsell-paid))
    (assert (prompt-shown))
    (printout t "ACTION: upsell-paid" crlf))

;;; ------------------------------------------------------------
;;; Upsell paid -> premium
;;; ------------------------------------------------------------
(defrule upsell-to-premium
    (declare (salience 40))
    (user-tier paid)
    (session-count ?s)
    (test (>= ?s 20))
    (feature-usage high)
    (not (prompt-shown))
    (not (prompt-suppressed))
    =>
    (assert (show upsell-premium))
    (assert (prompt-shown))
    (printout t "ACTION: upsell-premium" crlf))

;;; ------------------------------------------------------------
;;; Share credits for users who haven't shared much
;;; ------------------------------------------------------------
(defrule offer-share-credit
    (declare (salience 30))
    (social-shares ?n)
    (test (< ?n 3))
    (not (prompt-shown))
    (not (prompt-suppressed))
    =>
    (assert (show share-credit))
    (assert (prompt-shown))
    (printout t "ACTION: share-credit" crlf))
