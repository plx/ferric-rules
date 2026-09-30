(defrule probe =>
(printout t "e:[" (format nil "%e|%.0e|%.2e|%.2e|%.2e" 12345.0 9.9 1e-5 1e99 1e100) "]" crlf)
(printout t "zero:[" (format nil "%010.2e|%08.2f" -0.0 -0.0) "]" crlf)
(printout t "ties:[" (format nil "%.0f|%.0f|%.0f|%.0f" 2.5 3.5 -2.5 -3.5) "]" crlf)
(printout t "subnormal:[" (format nil "%.6e" 5e-324) "]" crlf)
)
