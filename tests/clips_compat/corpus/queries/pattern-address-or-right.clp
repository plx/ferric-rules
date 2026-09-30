;; Issue #328: assigned-pattern addresses in RHS expressions.
(deffacts seed (head) (padding) (right) (tail) (gate))
(defrule probe
  ?first <- (head)
  (or
    (and (not (blocked)) (left))
    (and (exists (gate)) (padding) (right)))
  ?last <- (tail)
  =>
  (printout t (fact-index ?first) ":" (fact-relation ?last) ":"
    (fact-index ?last) crlf))
