(deftemplate item (multislot left) (slot key) (multislot right))
(deffacts input (item (left a marker b marker c) (key retained) (right x div y div z)))
(defrule probe (item (right $?rl div $?rr) (key ?key) (left $?ll marker $?lr)) => (printout t ?key " " ?ll "|" ?lr " :: " ?rl "|" ?rr crlf))
