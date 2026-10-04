(deffacts seed (v 1 2 3))
(defrule partitions (v $?a $?b) => (printout t (length$ ?a) crlf))
