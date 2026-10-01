
(defrule probe =>
(printout t "[" (implode$ (create$ 1.0e-4 1.0e-5 1.0e14 1.0e15 1.0e16 1.2345678901234567e30 -1.0e-20 5.0e-324)) "]" crlf)
)
