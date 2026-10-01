;; Issue #328: a saved pattern address stays stale after retraction, and a
;; later assertion receives the next fact index.
(defglobal ?*saved* = FALSE)
(deffacts seed (sample a))
(defrule capture ?f <- (sample a) =>
  (bind ?copy ?f)
  (bind ?*saved* ?copy)
  (printout t "live:" (fact-index ?copy) ":" (fact-existp ?copy) crlf)
  (retract ?f)
  (printout t "stale:" (fact-index ?f) ":" (fact-existp ?copy) crlf)
  (assert (sample b)))
(defrule resumed ?fresh <- (sample b) =>
  (printout t "resumed:" (fact-index ?*saved*) ":" (fact-existp ?*saved*)
    ":" (fact-index ?fresh) crlf))
