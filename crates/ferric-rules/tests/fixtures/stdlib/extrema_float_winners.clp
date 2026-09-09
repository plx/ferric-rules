
(defrule probe =>
 (printout t (min 4 2.5 8) ":" (floatp (min 4 2.5 8)) crlf)
 (printout t (max 4 2 8.5) ":" (floatp (max 4 2 8.5)) crlf))
