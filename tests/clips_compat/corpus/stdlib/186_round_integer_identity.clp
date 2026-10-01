(defrule probe =>
 (printout t (round 0) ":" (round -0.0) ":" (round 7) ":" (round -7.0) crlf)
 (printout t (round 9007199254740993) ":" (round -9007199254740993) crlf)
 (printout t (round 9223372036854775807) ":" (round -9223372036854775808) crlf)
 (printout t (integerp (round 2.5)) ":" (integerp (round 2)) crlf))
