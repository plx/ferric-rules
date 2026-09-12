(deftemplate item (slot value))
(deffacts input (item (value a)))
(defrule probe (item (value )) => (printout t "matched" crlf))
