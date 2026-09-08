;; Issue #328: assigned-pattern addresses in RHS expressions.
(defglobal ?*saved* = FALSE)
(deffacts seed (sample a))
(defrule capture ?f <- (sample a) =>
  (bind ?copy ?f)
  (bind ?*saved* ?copy)
  (printout t "live:" (fact-index ?copy) ":" (fact-existp ?copy) crlf)
  (retract ?f)
  (printout t "stale:" (fact-index ?f) ":" (fact-existp ?copy) crlf))
;; RESUME AFTER RETRACTION
;; After capture fires, assert (sample b), then load the rule below and run.
(defrule resumed ?fresh <- (sample b) =>
  (printout t "resumed:" (fact-index ?*saved*) ":" (fact-existp ?*saved*)
    ":" (fact-index ?fresh) crlf))
