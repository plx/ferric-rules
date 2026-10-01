;; str-cat and sym-cat join raw text (quotes and backslashes stay as they
;; are) and spell FLOATs like printout; format %s writes a STRING's text.
;; Level: boundary
;; Covers: str-cat, sym-cat, format
(defrule probe =>
  (printout t (str-cat "a\"b" "|" "a\\b" "|two words") crlf)
  (printout t (sym-cat "a\"b" "|" "a\\b" "|two words") " " (symbolp (sym-cat "a" "b")) crlf)
  (printout t (format nil "%s" "two words") crlf)
  (printout t (str-cat 1.0e20) " " (sym-cat 1.0e20) " " (str-cat 0.1 2.5 -0.0) crlf))
