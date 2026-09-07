(deftemplate item (multislot tags))
(deffacts input (item (tags 1 2.5 "x")))
(defrule probe (item (tags $?values)) => (printout t (length$ ?values) ":" (integerp (nth$ 1 ?values)) ":" (floatp (nth$ 2 ?values)) ":" (stringp (nth$ 3 ?values)) crlf))
