(deffacts seed (row "a" "two words" "" 3))
(defrule exercise (row $?fields) => (printout t ?fields crlf))
