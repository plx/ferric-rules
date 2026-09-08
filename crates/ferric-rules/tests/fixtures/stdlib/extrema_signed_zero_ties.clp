
(defrule probe =>
 (printout t (min -0.0 0.0) ":" (min 0.0 -0.0) crlf)
 (printout t (max -0.0 0.0) ":" (max 0.0 -0.0) crlf)
 (printout t (min 0 -0.0) ":" (integerp (min 0 -0.0)) crlf)
 (printout t (max 0 -0.0) ":" (integerp (max 0 -0.0)) crlf))
