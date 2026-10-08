(deftemplate item (slot v))
(deffacts seed (item (v 1)))
(defrule r ?f <- (item (v 1)) =>
  (bind ?i 9)
  (modify ?i (v 2))
  (duplicate ?i (v 3))
  (printout t (fact-existp ?f) ":continued" crlf))
