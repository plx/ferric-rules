(deftemplate item (multislot left) (slot key) (multislot right))
(deffacts input (item (left a marker b marker c) (key retained) (right x div y div z)))
(defrule probe (item (left $?ll marker $?lr) (key ?key) (right $?rl div $?rr)) => (printout t ?key " " ?ll "|" ?lr " :: " ?rl "|" ?rr crlf))
