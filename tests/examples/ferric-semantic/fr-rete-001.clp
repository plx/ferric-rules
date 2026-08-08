; FR-RETE-001: duplicate assertions are rejected by default, can be enabled,
; and report the previous policy value when toggled.
(deffacts startup
  (start))

(defrule drive-duplication-policy
  ?start <- (start)
  =>
  (retract ?start)
  (printout t "default=" (get-fact-duplication) crlf)
  (assert (item default))
  (assert (item default))
  (printout t "enable-old=" (set-fact-duplication TRUE) crlf)
  (assert (item enabled))
  (assert (item enabled))
  (printout t "disable-old=" (set-fact-duplication FALSE) crlf)
  (assert (item enabled)))

(defrule observe-item
  (item ?kind)
  =>
  (printout t "item=" ?kind crlf))
