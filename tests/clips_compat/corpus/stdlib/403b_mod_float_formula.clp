; A FLOAT operand gives a - trunc(a / b) * b, which differs from C's fmod
; when the quotient is inexact or overflows.
(defrule check =>
  (printout t (mod 5.3 0.1) " " (mod 1e308 1e-308) " " (mod -7.5 2) " "
    (mod 7 -2.5) " " (mod 1e20 3.0) crlf))
