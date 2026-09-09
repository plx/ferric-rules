(deftemplate item (slot value))
(deffacts input (item (value a)))
(defrule probe (item (value $?x)) => (printout t "matched" crlf))
