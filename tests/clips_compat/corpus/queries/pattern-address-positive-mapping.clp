;; Issue #328: assigned-pattern addresses in RHS expressions.
(deffacts seed (head 10) (unassigned 11) (gate) (tail 20))
(defrule probe
  ?first <- (head 10)
  (not (blocked))
  (unassigned 11)
  (exists (gate))
  (test (= 1 1))
  ?last <- (tail 20)
  =>
  (printout t (fact-index ?first) ":" (fact-relation ?first) ":"
    (fact-index ?last) ":" (fact-relation ?last) crlf))
