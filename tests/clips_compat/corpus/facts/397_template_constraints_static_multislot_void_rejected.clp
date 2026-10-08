;; A static multislot default is evaluated once at definition; a void element
;; (here from printout) does not match the slot's allowed types, so CLIPS
;; reports CSTRNCHK1 after the output and leaves the template undefined.
;; Level: boundary
;; Covers: deftemplate, multislot, default, printout
(deftemplate a (multislot m (default 1 (printout t x crlf) 2)))
