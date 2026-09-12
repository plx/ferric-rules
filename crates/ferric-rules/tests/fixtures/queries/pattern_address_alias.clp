;; Issue #328: assigned-pattern addresses in RHS expressions.
(deftemplate item (slot value))
(deffacts seed (item (value 17)))
(defrule probe ?f <- (item (value 17)) =>
  (bind ?copy ?f)
  (bind ?f 42)
  (printout t "bound:" ?f ":" (fact-index ?copy) ":"
    (fact-relation ?copy) ":" (fact-slot-value ?copy value) ":"
    (fact-existp ?copy) crlf))
