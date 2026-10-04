(deftemplate item (slot v))
(deffacts seed (item (v 1)))
(defrule r (item) =>
  (printout t (fact-existp 9) " " (fact-relation 9) " " (fact-slot-names 9) crlf)
  (printout t (fact-slot-value 9 v) crlf)
  (retract 9)
  (printout t "continued" crlf))
