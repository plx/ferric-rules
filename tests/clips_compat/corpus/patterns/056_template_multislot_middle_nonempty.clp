(deftemplate item (multislot tags))
(deffacts input (item (tags head a b tail)))
(defrule probe (item (tags head $?values tail)) => (printout t (length$ ?values) ":" ?values crlf))
