(deftemplate item (slot value))
(deffacts input (item (value a)))
(defrule probe (item (value a b)) => (printout t "matched" crlf))
