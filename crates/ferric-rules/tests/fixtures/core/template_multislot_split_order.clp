(deftemplate item (multislot tags))
(deffacts input (item (tags a marker b marker c)))
(defrule probe (item (tags $?left marker $?right)) => (printout t ?left "|" ?right crlf))
