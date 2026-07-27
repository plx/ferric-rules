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
