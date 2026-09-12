
(defrule probe =>
 (printout t (min 2 2.0) ":" (integerp (min 2 2.0)) crlf)
 (printout t (min 2.0 2) ":" (floatp (min 2.0 2)) crlf)
 (printout t (max 2 2.0) ":" (integerp (max 2 2.0)) crlf)
 (printout t (max 2.0 2) ":" (floatp (max 2.0 2)) crlf)
 (printout t (min 9.0 2 2.0 3) ":" (integerp (min 9.0 2 2.0 3)) crlf)
 (printout t (max -9.0 2 2.0 1) ":" (integerp (max -9.0 2 2.0 1)) crlf))
