(defrule probe =>
 (printout t "[" (sub-string 0 2 abc) "]:[" (sub-string 2 9 abc) "]:["
  (sub-string 3 2 abc) "]:[" (sub-string 0 2 TRUE) "]:["
  (sub-string -2 99 FALSE) "]:" (stringp (sub-string 0 2 abc)) crlf))
