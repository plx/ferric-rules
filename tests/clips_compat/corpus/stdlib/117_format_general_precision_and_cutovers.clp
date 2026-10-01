(defrule probe =>
(printout t "g0:[" (format nil "%.0g|%.0g|%.0g" 1.0 9.9 -0.0) "]" crlf)
(printout t "g3:[" (format nil "%.3g|%.3g|%.3g|%.3g" 999.4 999.5 0.00009994 0.00009995) "]" crlf)
(printout t "default:[" (format nil "%g|%g|%g|%g" 999999.4 999999.5 0.0001 0.00001) "]" crlf)
(printout t "subnormal:[" (format nil "%.6g|%g" 5e-324 1.7976931348623157e308) "]" crlf)
)
