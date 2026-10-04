(deftemplate item (slot v))
(deffacts seed (item (v 1)) (item (v 2)))
(defrule run ?f <- (item (v 2)) =>
  (printout t (fact-relation (fact-index ?f)) ":" (fact-slot-value (fact-index ?f) v) crlf)
  (printout t (fact-existp 4294967299) ":" (fact-relation 4294967299) crlf)
  (retract 4294967299)
  (printout t (fact-existp ?f) ":continued" crlf))
