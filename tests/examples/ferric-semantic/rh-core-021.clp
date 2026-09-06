; RH-CORE-021: repeated variable constraints preserve symbol versus string identity.
(deffacts seed (pair red red) (pair red blue) (pair "red" red) (pair "blue" "blue"))
(defrule equal-fields (pair ?v ?v) => (printout t ?v crlf) (assert (result ?v)))
